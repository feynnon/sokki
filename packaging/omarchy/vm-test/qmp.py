#!/usr/bin/env python3
"""Drive the Try Omarchy VM through QEMU's QMP socket.

  qmp.py shot OUT.png          screenshot
  qmp.py keys super-ret ...    key combos (QEMU qcode names joined by '-')
  qmp.py type 'some text'      type text (US layout keys)
  qmp.py scroll 5              mouse wheel at the screen centre (negative: up)
"""
import glob, json, socket, sys, time

def sock():
    path = sorted(glob.glob("/tmp/omarchy-qemu-gpu.*/qmp.sock"), key=lambda p: -__import__("os").path.getmtime(p))[0]
    s = socket.socket(socket.AF_UNIX); s.connect(path)
    f = s.makefile("rw")
    f.readline()
    cmd(f, "qmp_capabilities")
    return f

def cmd(f, name, **args):
    f.write(json.dumps({"execute": name, "arguments": args} if args else {"execute": name}) + "\n"); f.flush()
    while True:
        r = json.loads(f.readline())
        if "event" in r: continue
        if "error" in r: raise SystemExit(f"{name}: {r['error']}")
        return r

SHIFTED = {'!':'1','@':'2','#':'3','$':'4','%':'5','^':'6','&':'7','*':'8','(':'9',')':'0',
           '_':'minus','+':'equal','{':'bracket_left','}':'bracket_right','|':'backslash',
           ':':'semicolon','"':'apostrophe','<':'comma','>':'dot','?':'slash','~':'grave_accent'}
PLAIN = {' ':'spc','-':'minus','=':'equal','[':'bracket_left',']':'bracket_right','\\':'backslash',
         ';':'semicolon',"'":'apostrophe',',':'comma','.':'dot','/':'slash','`':'grave_accent','\n':'ret','\t':'tab'}

def keys_for(ch):
    if ch.isalpha() and ch.isascii():
        return (["shift"] if ch.isupper() else []) + [ch.lower()]
    if ch.isdigit(): return [ch]
    if ch in PLAIN: return [PLAIN[ch]]
    if ch in SHIFTED: return ["shift", SHIFTED[ch]]
    raise SystemExit(f"cannot type {ch!r}")

def send(f, combo):
    cmd(f, "send-key", keys=[{"type": "qcode", "data": k} for k in combo], **{"hold-time": 40})
    time.sleep(0.04)

f = sock()
op = sys.argv[1]
if op == "shot":
    ppm = sys.argv[2].rsplit(".",1)[0] + ".ppm"; cmd(f, "screendump", filename=ppm); __import__("subprocess").run(["sips","-s","format","png",ppm,"--out",sys.argv[2]],capture_output=True)
elif op == "keys":
    for c in sys.argv[2:]:
        send(f, [{"super":"meta_l","ret":"ret","esc":"esc"}.get(k, k) for k in c.split("-")]); time.sleep(0.2)
elif op == "scroll":
    n = int(sys.argv[2])
    # Pointer to the centre (absolute axes run 0..32767), then wheel notches.
    cmd(f, "input-send-event", events=[{"type": "abs", "data": {"axis": "x", "value": 16384}},
                                       {"type": "abs", "data": {"axis": "y", "value": 16384}}])
    button = "wheel-down" if n > 0 else "wheel-up"
    for _ in range(abs(n)):
        cmd(f, "input-send-event", events=[{"type": "btn", "data": {"down": True, "button": button}}])
        cmd(f, "input-send-event", events=[{"type": "btn", "data": {"down": False, "button": button}}])
        time.sleep(0.12)
elif op == "type":
    for ch in sys.argv[2]: send(f, keys_for(ch))
