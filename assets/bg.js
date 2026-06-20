// "you are here" topography, deterministic: seeded by the date so each day draws
// its own contour map, with the phase drifting through the day. Same instant
// always renders the same image.
(function () {
  const svg = document.getElementById("bg");
  const mulberry32 = (a) => () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };

  function draw() {
    const w = innerWidth,
      h = innerHeight;
    svg.setAttribute("viewBox", `0 0 ${w} ${h}`);
    const now = new Date(),
      startOfDay = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
    const dayN = Math.floor(startOfDay / 86400000),
      dayFrac = (now.getTime() - startOfDay) / 86400000;
    const rnd = mulberry32((dayN * 2654435761) >>> 0);
    const sa = rnd() * 6.283,
      sb = rnd() * 6.283;
    const cx = w * (0.7 + 0.16 * rnd()),
      cy = h * (0.22 + 0.18 * rnd()),
      phase = dayFrac * 6.283;
    const rings = 9,
      step = Math.max(48, Math.min(w, h) * 0.075);
    let s = "";
    for (let k = 1; k <= rings; k++) {
      const r = k * step,
        segs = 84;
      let d = "";
      for (let i = 0; i <= segs; i++) {
        const a = (i / segs) * 6.283;
        const rr = r + Math.sin(a * 2 + sa + phase) * r * 0.07 + Math.sin(a * 3 + sb - phase + k * 0.4) * r * 0.045;
        d += (i ? "L" : "M") + (cx + Math.cos(a) * rr).toFixed(1) + " " + (cy + Math.sin(a) * rr).toFixed(1) + " ";
      }
      s += `<path d="${d}Z"/>`;
    }
    s += `<circle cx="${cx.toFixed(0)}" cy="${cy.toFixed(0)}" r="2.6"/>`;
    svg.innerHTML = s;
  }

  let r;
  addEventListener("resize", () => {
    clearTimeout(r);
    r = setTimeout(draw, 200);
  });
  draw();
  setInterval(draw, 60000);

  // Reveal once fonts are ready so there is no swap or layout shift on load.
  const reveal = () => document.documentElement.classList.add("fonts-ready");
  if (document.fonts && document.fonts.ready) document.fonts.ready.then(reveal);
  setTimeout(reveal, 800);

  // For anyone poking around in the console: there's a shell in here.
  const mono = "ui-monospace,SFMono-Regular,Menlo,monospace";
  console.log("%c$ ssh cwd.dev", `color:#1E22E6;font:600 13px/1.7 ${mono}`);
  console.log("%cthe same site, as a shell.", `color:#6B655C;font:12px ${mono}`);
})();
