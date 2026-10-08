use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
use image::ImageDecoder;
use sha2::{Digest, Sha256};
use std::{
	io::Cursor,
	net::{IpAddr, Ipv4Addr},
	path::Path,
	sync::OnceLock,
	time::Duration,
};
use url::Url;

const MAX_BYTES: usize = 5 * 1024 * 1024;

pub fn valid_hash(value: &str) -> bool {
	value.len() == 64
		&& value
			.bytes()
			.all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn public_v4(ip: Ipv4Addr) -> bool {
	let [a, b, _, _] = ip.octets();
	!ip.is_private()
		&& !ip.is_loopback()
		&& !ip.is_link_local()
		&& !ip.is_broadcast()
		&& !ip.is_documentation()
		&& !ip.is_unspecified()
		&& !ip.is_multicast()
		&& a != 0
		&& a < 224
		&& !(a == 100 && (64..=127).contains(&b))
		&& !(a == 198 && (b == 18 || b == 19))
		&& !(a == 192 && b == 0)
		&& !(a == 192 && b == 88 && ip.octets()[2] == 99)
}
pub fn public_ip(ip: IpAddr) -> bool {
	match ip {
		IpAddr::V4(ip) => public_v4(ip),
		IpAddr::V6(ip) => {
			if let Some(v4) = ip.to_ipv4_mapped() {
				return public_v4(v4);
			}
			let s = ip.segments();
			// Accept global unicast only; exclude documentation, transition and special-purpose ranges.
			s[0] & 0xe000 == 0x2000
				&& s[0] != 0x2002
				&& !(s[0] == 0x2001 && (s[1] < 0x0200 || s[1] == 0x0db8))
				&& !(s[0] == 0x3fff && s[1] < 0x1000)
		}
	}
}

pub async fn download_image(url: &str, directory: &Path) -> Result<String> {
	ensure!(url.len() <= 4096, "Image URL is too long");
	// Bound both active downloads and blocking decoders, including timed-out
	// decoders that may still be finishing their bounded amount of work.
	static SLOTS: OnceLock<std::sync::Arc<tokio::sync::Semaphore>> = OnceLock::new();
	let permit = tokio::time::timeout(
		Duration::from_secs(5),
		SLOTS
			.get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(4)))
			.clone()
			.acquire_owned(),
	)
	.await
	.context("Image queue is busy; try again shortly")??;
	let mut url = Url::parse(url).context("Enter a valid HTTPS image URL")?;
	let bytes = tokio::time::timeout(Duration::from_secs(15), async {
		for _ in 0..=3 {
			ensure!(
				url.scheme() == "https" && url.port_or_known_default() == Some(443),
				"Only HTTPS images on port 443 are supported"
			);
			ensure!(
				url.username().is_empty() && url.password().is_none(),
				"Image URLs cannot contain credentials"
			);
			let host = url.host_str().context("Image URL has no host")?.to_owned();
			let addresses: Vec<_> = tokio::net::lookup_host((host.as_str(), 443))
				.await?
				.collect();
			ensure!(
				!addresses.is_empty() && addresses.iter().all(|a| public_ip(a.ip())),
				"Image host must resolve only to public addresses"
			);
			let client = reqwest::Client::builder()
				.no_proxy()
				.redirect(reqwest::redirect::Policy::none())
				.resolve_to_addrs(&host, &addresses)
				.timeout(Duration::from_secs(10))
				.build()?;
			let response = client.get(url.clone()).send().await?;
			if response.status().is_redirection() {
				let location = response
					.headers()
					.get(reqwest::header::LOCATION)
					.context("Redirect has no destination")?
					.to_str()?;
				url = url.join(location)?;
				continue;
			}
			ensure!(
				response.status().is_success(),
				"Image server returned {}",
				response.status()
			);
			ensure!(
				response
					.content_length()
					.is_none_or(|n| n <= MAX_BYTES as u64),
				"Image exceeds the 5 MiB limit"
			);
			let mut bytes = Vec::new();
			let mut stream = response.bytes_stream();
			while let Some(chunk) = stream.next().await {
				let chunk = chunk?;
				ensure!(
					bytes.len() + chunk.len() <= MAX_BYTES,
					"Image exceeds the 5 MiB limit"
				);
				bytes.extend_from_slice(&chunk);
			}
			return Ok(bytes);
		}
		bail!("Too many image redirects")
	})
	.await
	.context("Image download timed out")??;
	let png = tokio::time::timeout(
		Duration::from_secs(10),
		tokio::task::spawn_blocking(move || {
			let _permit = permit;
			normalize_image(&bytes)
		}),
	)
	.await
	.context("Image decoding timed out")???;
	let hash = format!("{:x}", Sha256::digest(&png));
	tokio::fs::create_dir_all(directory).await?;
	let target = directory.join(format!("{hash}.png"));
	let temp = directory.join(format!("{}.tmp", uuid::Uuid::new_v4()));
	tokio::fs::write(&temp, png).await?;
	if let Err(error) = tokio::fs::rename(&temp, &target).await {
		let _ = tokio::fs::remove_file(&temp).await;
		if !target.exists() {
			return Err(error.into());
		}
	}
	Ok(hash)
}

fn normalize_image(bytes: &[u8]) -> Result<Vec<u8>> {
	ensure!(bytes.len() <= MAX_BYTES, "Image exceeds the 5 MiB limit");
	let format = image::guess_format(bytes)?;
	ensure!(
		matches!(
			format,
			image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP
		),
		"Use a static PNG, JPEG, or WebP image"
	);
	let mut limits = image::Limits::default();
	limits.max_image_width = Some(4096);
	limits.max_image_height = Some(4096);
	limits.max_alloc = Some(128 * 1024 * 1024);
	let image = match format {
		image::ImageFormat::Png => {
			let decoder =
				image::codecs::png::PngDecoder::with_limits(Cursor::new(bytes), limits.clone())?;
			ensure!(!decoder.is_apng()?, "Animated images are not supported");
			decode(decoder, limits)?
		}
		image::ImageFormat::WebP => {
			let decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))?;
			ensure!(
				!decoder.has_animation(),
				"Animated images are not supported"
			);
			decode(decoder, limits)?
		}
		image::ImageFormat::Jpeg => decode(
			image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes))?,
			limits,
		)?,
		_ => unreachable!(),
	}
	.thumbnail(1024, 1024);
	let mut output = Cursor::new(Vec::new());
	image.write_to(&mut output, image::ImageFormat::Png)?;
	Ok(output.into_inner())
}
fn decode(mut decoder: impl ImageDecoder, limits: image::Limits) -> Result<image::DynamicImage> {
	decoder.set_limits(limits)?;
	let orientation = decoder.orientation()?;
	let mut image = image::DynamicImage::from_decoder(decoder)?;
	image.apply_orientation(orientation);
	Ok(image)
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn reject_internal_and_transition_addresses() {
		for address in [
			"127.0.0.1",
			"10.0.0.1",
			"172.16.0.1",
			"192.168.0.1",
			"169.254.169.254",
			"100.64.0.1",
			"0.0.0.0",
			"224.0.0.1",
			"192.88.99.1",
			"::1",
			"::ffff:127.0.0.1",
			"fc00::1",
			"fe80::1",
			"2001:db8::1",
			"2002:7f00:1::",
			"3fff::1",
		] {
			assert!(!public_ip(address.parse().unwrap()), "{address}");
		}
		assert!(public_ip("1.1.1.1".parse().unwrap()));
		assert!(public_ip("2606:4700:4700::1111".parse().unwrap()));
		assert!(public_ip("2001:4860:4860::8888".parse().unwrap()));
	}
	#[test]
	fn reject_executable_content() {
		assert!(normalize_image(b"<svg onload='bad()'></svg>").is_err());
	}
	#[test]
	fn normalize_preserves_transparency() {
		let input = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 0]));
		let mut bytes = Cursor::new(Vec::new());
		input.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
		let output = normalize_image(bytes.get_ref()).unwrap();
		assert_eq!(
			image::load_from_memory(&output)
				.unwrap()
				.to_rgba8()
				.get_pixel(0, 0)
				.0[3],
			0
		);
	}
	#[test]
	fn animation_like_bytes_in_static_metadata_are_not_animation() {
		let image = image::RgbImage::from_pixel(2, 2, image::Rgb([1, 2, 3]));
		let mut bytes = Cursor::new(Vec::new());
		image
			.write_to(&mut bytes, image::ImageFormat::Jpeg)
			.unwrap();
		let mut jpeg = bytes.into_inner();
		let text = b"This photograph has ANIM and acTL in its comment.";
		let mut comment = vec![0xff, 0xfe];
		comment.extend_from_slice(&((text.len() + 2) as u16).to_be_bytes());
		comment.extend_from_slice(text);
		jpeg.splice(2..2, comment);
		assert!(normalize_image(&jpeg).is_ok());
	}
	#[test]
	fn rejects_actual_apng_control_chunks() {
		fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
			let mut bytes = Vec::new();
			bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
			bytes.extend_from_slice(kind);
			bytes.extend_from_slice(data);
			let mut crc = 0xffff_ffffu32;
			for byte in &bytes[4..] {
				crc ^= *byte as u32;
				for _ in 0..8 {
					crc = (crc >> 1) ^ if crc & 1 == 1 { 0xedb8_8320 } else { 0 };
				}
			}
			bytes.extend_from_slice(&(!crc).to_be_bytes());
			bytes
		}
		let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]));
		let mut bytes = Cursor::new(Vec::new());
		image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
		let mut png = bytes.into_inner();
		let mut controls = chunk(b"acTL", &[0, 0, 0, 1, 0, 0, 0, 0]);
		controls.extend(chunk(
			b"fcTL",
			&[
				0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 10, 0, 0,
			],
		));
		png.splice(33..33, controls);
		assert!(
			normalize_image(&png)
				.unwrap_err()
				.to_string()
				.contains("Animated")
		);
	}
}
