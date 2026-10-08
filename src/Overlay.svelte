<script lang="ts">
  import Stage from "./Stage.svelte";
  import type { OverlayEvent } from "./lib/types";
  const assetBase = new URL("./", location.href).toString();
  function subscribe(callback: (event: OverlayEvent) => void) {
    let socket: WebSocket | null = null;
    let timer: ReturnType<typeof setTimeout>;
    let disposed = false;
    let attempts = 0;
    const connect = () => {
      if (disposed) return;
      const url = new URL("events", assetBase);
      url.protocol = "ws:";
      socket = new WebSocket(url);
      socket.onopen = () => {
        attempts = 0;
      };
      socket.onmessage = (event) => {
        try {
          callback(JSON.parse(event.data));
        } catch {
          socket?.close();
        }
      };
      socket.onclose = () => {
        callback({ type: "stop_speech" });
        callback({ type: "hide_image" });
        if (!disposed)
          timer = setTimeout(
            connect,
            Math.min(30000, 1000 * 2 ** Math.min(attempts++, 5)),
          );
      };
    };
    connect();
    return () => {
      disposed = true;
      clearTimeout(timer);
      socket?.close();
    };
  }
</script>

<main><Stage {assetBase} {subscribe} muted={false} /></main>

<style>
  :global(html),
  :global(body),
  :global(#app) {
    margin: 0;
    width: 100%;
    height: 100%;
    background: transparent;
    overflow: hidden;
  }
  main {
    width: 100vw;
    height: 100vh;
  }
</style>
