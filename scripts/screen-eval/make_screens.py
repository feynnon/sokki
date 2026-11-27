#!/usr/bin/env python3
"""Made-up test screens for the screen-recording compression eval.

Writes four screens as Retina PNGs (3456x2234, a 14" MacBook Pro at its
default scale) plus the words each one shows, into
$SOKKI_EVALS/screen-capture-made-up/ (default ~/sokki-evals):

  slides.png    a presentation slide with large and small text
  code.png      a dark code editor, 12 px monospace
  document.png  a document page, 11 px and 12 px body text (11/12 pt at 100 %)
  web.png       a tall web page (4 screens high), scrolled through by bench.swift
  truth.json    per screen, the words in its small text (what OCR must find)

All text is generated from a fixed word list with a fixed seed; nothing comes
from a real screen. Rendered with headless Chrome (no window).

Usage: python3 scripts/screen-eval/make_screens.py
"""

import html
import json
import os
import random
import subprocess
import sys
import tempfile
from pathlib import Path

SEED = 20261127
W, H = 1728, 1117  # logical points; Chrome renders at 2x
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

WORDS = """apple river budget north window planet garden silver orange market
station paper winter mountain coffee rocket letter bridge yellow kitchen
monday report travel number signal pocket forest island simple future
copper engine falcon harbor jacket lemon marble needle oyster pepper quarter
saddle ticket velvet walnut anchor basket candle dragon feather glacier
hammer ivory jungle kettle ladder meadow nickel parrot rabbit tunnel violet
wizard zipper meeting project invoice weekly growth review launch design
customer revenue backlog sprint release deadline estimate pricing contract""".split()


def sentence(rng, n):
    words = [rng.choice(WORDS) for _ in range(n)]
    # Numbers are where blurry text fails first (6/8, 1/7, 0/O).
    for _ in range(max(1, n // 6)):
        words[rng.randrange(n)] = str(rng.randint(10, 9999))
    words[0] = words[0].capitalize()
    return " ".join(words) + "."


def page(body, css):
    return f"""<!doctype html><html><head><meta charset="utf-8"><style>
html,body{{margin:0;padding:0}} body{{font-family:-apple-system,"Helvetica Neue",sans-serif}}
{css}</style></head><body>{body}</body></html>"""


def slides(rng):
    bullets = [sentence(rng, 7) for _ in range(4)]
    foot = [sentence(rng, 12) for _ in range(2)]
    body = (
        '<div class="s"><h1>Quarterly Planning: Garden Rocket</h1><ul>'
        + "".join(f"<li>{html.escape(b)}</li>" for b in bullets)
        + "</ul>"
        + "".join(f'<p class="f">{html.escape(f)}</p>' for f in foot)
        + "</div>"
    )
    css = """.s{width:1728px;height:1117px;box-sizing:border-box;padding:90px 120px;
background:linear-gradient(135deg,#1d2b53,#3a1d53);color:#fff}
h1{font-size:64px;margin:0 0 50px} li{font-size:34px;margin:18px 0}
.f{font-size:14px;color:#cfd3ff;margin:6px 0}"""
    small = foot
    return page(body, css), small


def code(rng):
    lines = []
    for i in range(60):
        name = rng.choice(WORDS)
        other = rng.choice(WORDS)
        n = rng.randint(10, 9999)
        kind = i % 4
        if kind == 0:
            lines.append(f'<span class="k">fn</span> {name}_{other}(count: u32) -&gt; u32 {{')
        elif kind == 1:
            lines.append(f'    <span class="k">let</span> {name} = count * {n};')
        elif kind == 2:
            lines.append(f'    <span class="c">// {other} {name} {rng.choice(WORDS)} {n}</span>')
        else:
            lines.append(f"    {name} + {n} }}")
    text_lines = [
        html.unescape(
            l.replace('<span class="k">', "").replace('<span class="c">', "").replace("</span>", "")
        )
        for l in lines
    ]
    body = (
        '<div class="e"><div class="side">'
        + "".join(f"<div>{rng.choice(WORDS)}.rs</div>" for _ in range(30))
        + '</div><pre class="ed">'
        + "\n".join(f'<span class="ln">{i + 1:3d}</span>  {l}' for i, l in enumerate(lines))
        + "</pre></div>"
    )
    css = """.e{display:flex;width:1728px;height:1117px;background:#1e1e1e;color:#d4d4d4}
.side{width:240px;background:#252526;font-size:13px;padding:10px;color:#ccc;line-height:22px}
.ed{margin:0;padding:10px 16px;font:12px/18px Menlo,monospace}
.k{color:#569cd6}.c{color:#6a9955}.ln{color:#858585}"""
    return page(body, css), text_lines


def document(rng):
    paras11 = [sentence(rng, 40) for _ in range(5)]
    paras12 = [sentence(rng, 40) for _ in range(5)]
    body = (
        '<div class="bg"><div class="pg"><h2>Weekly Report: Harbor Walnut</h2>'
        + '<p class="t">11 pt</p>'
        + "".join(f'<p class="p11">{html.escape(p)}</p>' for p in paras11)
        + '<p class="t">12 pt</p>'
        + "".join(f'<p class="p12">{html.escape(p)}</p>' for p in paras12)
        + "</div></div>"
    )
    css = """.bg{width:1728px;height:1117px;background:#e8e8e8;padding-top:30px;box-sizing:border-box}
.pg{width:820px;margin:0 auto;background:#fff;padding:50px 70px;height:1060px;box-sizing:border-box;
font-family:"Times New Roman",Georgia,serif;color:#111}
h2{font-size:20px;margin:0 0 12px}.t{font:bold 10px sans-serif;color:#888;margin:8px 0 2px}
.p11{font-size:11px;line-height:1.35;margin:0 0 8px}.p12{font-size:12px;line-height:1.35;margin:0 0 8px}"""
    return page(body, css), paras11 + paras12


def web(rng):
    cards = []
    small = []
    for i in range(24):
        title = " ".join(rng.choice(WORDS) for _ in range(3)).title()
        text = sentence(rng, 26)
        small.append(text)
        cards.append(
            f'<div class="card"><div class="img" style="background:hsl({rng.randint(0,360)},55%,60%)"></div>'
            f"<h3>{html.escape(title)}</h3><p>{html.escape(text)}</p></div>"
        )
    body = (
        '<div class="nav">Walnut News · Planet · Market · Travel · Design</div><div class="grid">'
        + "".join(cards)
        + "</div>"
    )
    css = """body{background:#fafafa;width:1728px}
.nav{height:56px;background:#fff;border-bottom:1px solid #ddd;font-size:15px;line-height:56px;padding:0 40px}
.grid{display:grid;grid-template-columns:repeat(3,1fr);gap:28px;padding:28px 120px}
.card{background:#fff;border-radius:10px;box-shadow:0 1px 3px #0002;padding:0 0 14px}
.img{height:170px;border-radius:10px 10px 0 0}
h3{font-size:17px;margin:12px 16px 6px}p{font-size:13px;line-height:1.45;margin:0 16px;color:#333}"""
    return page(body, css), small


def render(html_text, out, height):
    with tempfile.NamedTemporaryFile("w", suffix=".html", delete=False) as f:
        f.write(html_text)
        src = f.name
    with tempfile.TemporaryDirectory() as profile:
        subprocess.run(
            [
                CHROME,
                "--headless=new",
                "--disable-gpu",
                "--hide-scrollbars",
                f"--user-data-dir={profile}",
                "--force-device-scale-factor=2",
                f"--window-size={W},{height}",
                f"--screenshot={out}",
                "file://" + src,
            ],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    os.unlink(src)


def main():
    root = Path(os.environ.get("SOKKI_EVALS", Path.home() / "sokki-evals")) / "screen-capture-made-up"
    root.mkdir(parents=True, exist_ok=True)
    rng = random.Random(SEED)
    truth = {}
    for name, make, height in [
        ("slides", slides, H),
        ("code", code, H),
        ("document", document, H),
        ("web", web, H * 4),
    ]:
        text, small = make(rng)
        render(text, str(root / f"{name}.png"), height)
        truth[name] = small
        print(name, root / f"{name}.png")
    (root / "truth.json").write_text(json.dumps(truth, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
