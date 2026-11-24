// sokki.org: the copy button and the dictation line in the hero. Page views are counted by website-counter/ (no script here).
document.querySelectorAll("[data-copy]").forEach((btn) => {
  btn.addEventListener("click", async () => {
    const text = document.getElementById(btn.dataset.copy).textContent.trim();
    try {
      await navigator.clipboard.writeText(text);
      btn.textContent = "Copied";
    } catch {
      btn.textContent = "Select and copy";
    }
    setTimeout(() => (btn.textContent = "Copy command"), 1800);
  });
});

const typed = document.querySelector("[data-typed]");
const timer = document.querySelector("[data-timer]");
const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
if (typed && !still) {
  const lines = [
    "Can we move the review to Thursday?",
    "Send Anna the slides before lunch.",
    "Kannst du mir die Zahlen bis Freitag schicken?",
  ];
  let line = 0;
  let seconds = 4;
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  (async () => {
    for (;;) {
      const text = lines[line];
      typed.textContent = "";
      for (const word of text.split(" ")) {
        typed.textContent += (typed.textContent ? " " : "") + word;
        await wait(170);
      }
      await wait(2600);
      line = (line + 1) % lines.length;
    }
  })();
  if (timer) {
    setInterval(() => {
      seconds = seconds >= 59 ? 0 : seconds + 1;
      timer.textContent = `0:${String(seconds).padStart(2, "0")}`;
    }, 1000);
  }
}
