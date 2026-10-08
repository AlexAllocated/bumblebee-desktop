import { setup } from "xstate";

export const overlayMachine = setup({}).createMachine({
	id: "bumblebee.overlay",
	initial: "idle",
	states: {
		idle: { on: { START: "starting" } },
		starting: { on: { READY: "ready", FAIL: "failed", DISPOSE: "disposing" } },
		ready: { on: { DISPOSE: "disposing", FAIL: "failed" } },
		disposing: { on: { DISPOSED: "disposed" } },
		disposed: { type: "final" },
		failed: { on: { DISPOSE: "disposing" } }
	}
});

export const surfaceMachine = setup({}).createMachine({
	id: "bumblebee.surface",
	initial: "idle",
	states: {
		idle: { on: { CREATE: "creating" } },
		creating: { on: { READY: "ready", FAIL: "failed", DISPOSE: "disposed" } },
		ready: {
			on: {
				RESIZE: "resizing",
				PERFORMANCE_CHANGE: "changingPerformance",
				DISPOSE: "disposed"
			}
		},
		resizing: { on: { READY: "ready", DISPOSE: "disposed" } },
		changingPerformance: { on: { READY: "ready", DISPOSE: "disposed" } },
		disposed: { type: "final" },
		failed: { on: { DISPOSE: "disposed" } }
	}
});

export const audioMachine = setup({}).createMachine({
	id: "bumblebee.audio",
	initial: "idle",
	states: {
		idle: { on: { CREATE: "creating" } },
		creating: { on: { SUSPENDED: "suspended", RUNNING: "running", FAIL: "failed" } },
		suspended: { on: { UNLOCK: "unlocking", DISPOSE: "disposed" } },
		unlocking: { on: { RUNNING: "running", SUSPENDED: "suspended", FAIL: "failed" } },
		running: { on: { SUSPENDED: "suspended", DISPOSE: "disposed" } },
		disposed: { type: "final" },
		failed: { on: { DISPOSE: "disposed" } }
	}
});

export const chatBubbleLayerMachine = setup({}).createMachine({
	id: "bumblebee.chatBubbles",
	initial: "disabled",
	states: {
		disabled: { on: { ENABLE: "ready", DISPOSE: "disposed" } },
		ready: {
			on: {
				START: "active",
				DISPOSE: "disposed"
			}
		},
		active: {
			on: {
				START: "active",
				END: "active",
				IDLE: "ready",
				DISPOSE: "disposed"
			}
		},
		disposed: { type: "final" }
	}
});

export const bumblebeeMachine = setup({}).createMachine({
	id: "bumblebee.actor",
	initial: "loading",
	states: {
		loading: { on: { READY: "hidden", FAIL: "failed", DISPOSE: "disposing" } },
		hidden: { on: { SHOW: "visible", DISPOSE: "disposing" } },
		visible: {
			type: "parallel",
			on: { HIDE: "hidden", DISPOSE: "disposing" },
			states: {
				stance: {
					initial: "standing",
					states: {
						standing: { on: { FLY: "idleFlying", MOVE: "walking", ERROR: "error" } },
						idleFlying: { on: { LAND: "standing", MOVE: "activeFlying", ERROR: "error" } },
						walking: { on: { STOP: "standing", FLY: "activeFlying", ERROR: "error" } },
						activeFlying: { on: { STOP: "idleFlying", LAND: "walking", ERROR: "error" } },
						error: { on: { STOP: "standing" } }
					}
				},
				speech: {
					initial: "silent",
					states: {
						silent: { on: { TALK: "talking" } },
						talking: { on: { SHUTUP: "silent" } }
					}
				},
				movement: {
					initial: "idle",
					states: {
						idle: { on: { MOVE_START: "moving" } },
						moving: { on: { MOVE_END: "idle" } }
					}
				}
			}
		},
		disposing: { on: { DISPOSED: "disposed" } },
		disposed: { type: "final" },
		failed: { on: { DISPOSE: "disposing" } }
	}
});

export const puppetMachine = setup({}).createMachine({
	id: "bumblebee.puppet",
	initial: "loading",
	states: {
		loading: { on: { READY: "hidden", FAIL: "failed", DISPOSE: "disposing" } },
		hidden: { on: { SHOW: "showing", DISPOSE: "disposing" } },
		showing: { on: { SHOWN: "visible", HIDE: "hiding", DISPOSE: "disposing" } },
		visible: {
			type: "parallel",
			on: { HIDE: "hiding", DISPOSE: "disposing" },
			states: {
				speech: {
					initial: "silent",
					states: {
						silent: { on: { TALK: "talking" } },
						talking: { on: { SHUTUP: "silent" } }
					}
				},
				movement: {
					initial: "idle",
					states: {
						idle: { on: { MOVE: "moving" } },
						moving: { on: { MOVED: "idle" } }
					}
				},
				facing: {
					initial: "auto",
					states: {
						auto: { on: { FACE_LEFT: "left", FACE_RIGHT: "right" } },
						left: { on: { FACE_AUTO: "auto", FACE_RIGHT: "right" } },
						right: { on: { FACE_AUTO: "auto", FACE_LEFT: "left" } }
					}
				}
			}
		},
		hiding: { on: { HIDDEN: "hidden", DISPOSE: "disposing" } },
		disposing: { on: { DISPOSED: "disposed" } },
		disposed: { type: "final" },
		failed: { on: { DISPOSE: "disposing" } }
	}
});
