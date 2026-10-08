const WS_HEARTBEAT_PING = "PING";
const WS_HEARTBEAT_PONG = "PONG";
const WEBSOCKET_OPEN_STATE = 1;

type ClientHeartbeatSocket = {
	send(data: string): unknown;
};

type ClientHeartbeatOptions = {
	socket: ClientHeartbeatSocket;
	timeoutMs: number;
	pingMessage?: string;
	pongMessage?: string;
	onTimeout: () => void;
};

type ServerHeartbeatOptions = {
	intervalMs: number;
	getIsAlive: () => boolean;
	setIsAlive: (alive: boolean) => void;
	isOpen: () => boolean;
	sendPing: (message: string) => void;
	terminate: () => void;
	pingMessage?: string;
	pongMessage?: string;
};

const createTimeoutController = (timeoutMs: number, onTimeout: () => void) => {
	let timer: ReturnType<typeof setTimeout> | null = null;

	return {
		reset() {
			if (timer) clearTimeout(timer);
			timer = setTimeout(() => {
				timer = null;
				onTimeout();
			}, timeoutMs);
		},
		clear() {
			if (!timer) return;
			clearTimeout(timer);
			timer = null;
		}
	};
};

const normalizeHeartbeatMessage = (data: unknown) => {
	if (typeof data === "string") return data;
	if (data instanceof ArrayBuffer) return new TextDecoder().decode(new Uint8Array(data));
	if (ArrayBuffer.isView(data)) {
		return new TextDecoder().decode(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
	}
	return String(data ?? "");
};

const createWebSocketClientHeartbeat = ({
	socket,
	timeoutMs,
	pingMessage = WS_HEARTBEAT_PING,
	pongMessage = WS_HEARTBEAT_PONG,
	onTimeout
}: ClientHeartbeatOptions) => {
	const timer = createTimeoutController(timeoutMs, onTimeout);

	return {
		handleMessage(data: unknown) {
			if (normalizeHeartbeatMessage(data) !== pingMessage) return false;
			timer.reset();
			socket.send(pongMessage);
			return true;
		},
		markAlive() {
			timer.reset();
		},
		stop() {
			timer.clear();
		}
	};
};

const createWebSocketServerHeartbeat = ({
	intervalMs,
	getIsAlive,
	setIsAlive,
	isOpen,
	sendPing,
	terminate,
	pingMessage = WS_HEARTBEAT_PING,
	pongMessage = WS_HEARTBEAT_PONG
}: ServerHeartbeatOptions) => {
	let interval: ReturnType<typeof setInterval> | null = null;

	const stop = () => {
		if (!interval) return;
		clearInterval(interval);
		interval = null;
	};

	const tick = () => {
		if (!isOpen()) {
			stop();
			return;
		}
		if (!getIsAlive()) {
			stop();
			terminate();
			return;
		}
		setIsAlive(false);
		sendPing(pingMessage);
	};

	return {
		start() {
			if (interval) return;
			setIsAlive(false);
			sendPing(pingMessage);
			interval = setInterval(tick, intervalMs);
		},
		handleMessage(data: unknown) {
			if (normalizeHeartbeatMessage(data) !== pongMessage) return false;
			setIsAlive(true);
			return true;
		},
		stop
	};
};

const toUint8Array = (input: ArrayBuffer | Uint8Array | ArrayBufferView) => {
	if (input instanceof Uint8Array) return input;
	if (input instanceof ArrayBuffer) return new Uint8Array(input);
	return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
};

const coerceWebSocketBinaryData = (input: unknown) => {
	if (input instanceof Uint8Array) return input;
	if (input instanceof ArrayBuffer) return new Uint8Array(input);
	if (ArrayBuffer.isView(input)) {
		return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
	}
	return null;
};

export {
	WS_HEARTBEAT_PING,
	WS_HEARTBEAT_PONG,
	WEBSOCKET_OPEN_STATE,
	coerceWebSocketBinaryData,
	createWebSocketClientHeartbeat,
	createWebSocketServerHeartbeat,
	toUint8Array
};
