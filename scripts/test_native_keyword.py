"""Exercise a real canceled Speech SDK request without credentials or audio devices."""
import ctypes
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest


RESOURCES = Path(__file__).resolve().parents[1] / "src-tauri/resources"
CORE = "libMicrosoft.CognitiveServices.Speech.core.so"
WRAPPER = "libbumblebee_speech_wrapper.so"


def canceled_request(native, shadow, model):
    # Reproduce linuxdeploy's old duplicate-core failure: the runtime loads,
    # but its delayed keyword extensions are absent beside this exact core.
    core = ctypes.CDLL(str(Path(shadow) / CORE), mode=ctypes.RTLD_LOCAL)
    api = ctypes.CDLL(str(Path(native) / WRAPPER), mode=ctypes.RTLD_LOCAL)
    api.bb_keyword_create.argtypes = [ctypes.c_char_p, ctypes.c_uint32, ctypes.c_uint8,
                                      ctypes.c_uint8, ctypes.POINTER(ctypes.c_void_p)]
    api.bb_keyword_create.restype = ctypes.c_int32
    api.bb_keyword_write.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32]
    api.bb_keyword_write.restype = ctypes.c_int32
    api.bb_keyword_poll.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_void_p,
                                    ctypes.c_uint32, ctypes.POINTER(ctypes.c_uint32)]
    api.bb_keyword_poll.restype = ctypes.c_int32
    api.bb_keyword_close.argtypes = [ctypes.c_void_p]
    api.bb_keyword_close.restype = ctypes.c_int32
    api.bb_last_error.argtypes = []
    api.bb_last_error.restype = ctypes.c_char_p
    handle = ctypes.c_void_p()
    assert api.bb_keyword_create(os.fsencode(model), 16000, 16, 1, ctypes.byref(handle)) == 0
    silence = ctypes.create_string_buffer(6400)
    assert api.bb_keyword_write(handle, silence, len(silence)) == 0
    output = ctypes.create_string_buffer(512)
    reason = ctypes.c_uint32()
    deadline = time.monotonic() + 5
    status = 1
    while status == 1 and time.monotonic() < deadline:
        status = api.bb_keyword_poll(handle, 100, output, len(output), ctypes.byref(reason))
    assert reason.value == 1, "Expected the real SDK's Canceled result"
    assert status == -2, "Cancellation must return an error, not a new recognition request"
    original = api.bb_last_error()
    assert original, "Cancellation lost its diagnostic"
    for _ in range(2):
        assert api.bb_keyword_poll(handle, 0, output, len(output), ctypes.byref(reason)) == -2
        assert api.bb_last_error() == original, "Repeated poll changed the original failure"
        assert api.bb_keyword_write(handle, silence, len(silence)) == -2
        assert api.bb_last_error() == original, "Write changed the original failure"
    assert api.bb_keyword_close(handle) == 0
    # Keep both dlopen handles alive through native teardown.
    assert core is not None


class NativeKeywordFailureTests(unittest.TestCase):
    @unittest.skipUnless(sys.platform == "linux" and (RESOURCES / "native" / WRAPPER).is_file(),
                         "requires prepared Linux native resources")
    def test_canceled_keyword_never_rearms_and_closes_without_hanging(self):
        with tempfile.TemporaryDirectory(prefix="bumblebee-keyword-failure-") as directory:
            shutil.copy2(RESOURCES / "native" / CORE, Path(directory) / CORE)
            env = os.environ.copy()
            env["LD_LIBRARY_PATH"] = os.pathsep.join(filter(None, [directory, env.get("LD_LIBRARY_PATH")]))
            result = subprocess.run([
                sys.executable, str(Path(__file__).resolve()), "--child", str(RESOURCES / "native"),
                directory, str(RESOURCES / "keyword_models/default/hey_bumblebee_default.table"),
            ], env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--child":
        canceled_request(*sys.argv[2:])
    else:
        unittest.main()
