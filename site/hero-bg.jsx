/* global React */
const { useEffect: useEffectBg, useRef: useRefBg } = React;

/* Animated agent network: nodes drift, edges pulse with traveling packets */
function HeroBackground() {
  const canvasRef = useRefBg(null);

  useEffectBg(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    let raf;
    let w = 0, h = 0;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);

    function resize() {
      const r = canvas.getBoundingClientRect();
      w = r.width; h = r.height;
      canvas.width = w * dpr; canvas.height = h * dpr;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }
    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);

    // Build nodes — fewer on small screens
    const NODE_COUNT = w < 700 ? 14 : 22;
    const nodes = Array.from({ length: NODE_COUNT }).map(() => ({
      x: Math.random() * w,
      y: Math.random() * h,
      vx: (Math.random() - 0.5) * 0.18,
      vy: (Math.random() - 0.5) * 0.18,
      r: 1.3 + Math.random() * 1.4,
      phase: Math.random() * Math.PI * 2,
    }));

    // Pre-compute static edges (proximity-based) — recomputed each frame for movement
    const MAX_DIST = w < 700 ? 140 : 180;

    // Packets travel along edges
    const packets = [];
    function spawnPacket() {
      // pick a random pair of nearby nodes
      const a = Math.floor(Math.random() * nodes.length);
      // find nearest neighbors
      const candidates = [];
      for (let i = 0; i < nodes.length; i++) {
        if (i === a) continue;
        const dx = nodes[i].x - nodes[a].x;
        const dy = nodes[i].y - nodes[a].y;
        const d2 = dx * dx + dy * dy;
        if (d2 < MAX_DIST * MAX_DIST) candidates.push(i);
      }
      if (!candidates.length) return;
      const b = candidates[Math.floor(Math.random() * candidates.length)];
      packets.push({ a, b, t: 0, speed: 0.004 + Math.random() * 0.006 });
    }
    const spawnTimer = setInterval(spawnPacket, 320);

    // Read accent CSS var
    function readAccent() {
      const s = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim();
      return s || "rgb(140, 220, 160)";
    }
    let accent = readAccent();
    const accentTimer = setInterval(() => { accent = readAccent(); }, 600);

    function draw(now) {
      ctx.clearRect(0, 0, w, h);

      // Drift nodes
      for (const n of nodes) {
        n.x += n.vx; n.y += n.vy;
        if (n.x < 0 || n.x > w) n.vx *= -1;
        if (n.y < 0 || n.y > h) n.vy *= -1;
        n.x = Math.max(0, Math.min(w, n.x));
        n.y = Math.max(0, Math.min(h, n.y));
      }

      // Edges
      for (let i = 0; i < nodes.length; i++) {
        for (let j = i + 1; j < nodes.length; j++) {
          const a = nodes[i], b = nodes[j];
          const dx = a.x - b.x, dy = a.y - b.y;
          const d = Math.hypot(dx, dy);
          if (d < MAX_DIST) {
            const alpha = (1 - d / MAX_DIST) * 0.12;
            ctx.strokeStyle = `oklch(0.78 0.16 145 / ${alpha.toFixed(3)})`;
            ctx.lineWidth = 0.6;
            ctx.beginPath();
            ctx.moveTo(a.x, a.y);
            ctx.lineTo(b.x, b.y);
            ctx.stroke();
          }
        }
      }

      // Packets
      for (let i = packets.length - 1; i >= 0; i--) {
        const p = packets[i];
        p.t += p.speed;
        if (p.t >= 1) { packets.splice(i, 1); continue; }
        const a = nodes[p.a], b = nodes[p.b];
        const x = a.x + (b.x - a.x) * p.t;
        const y = a.y + (b.y - a.y) * p.t;
        // glow
        const grad = ctx.createRadialGradient(x, y, 0, x, y, 14);
        grad.addColorStop(0, "oklch(0.78 0.16 145 / 0.55)");
        grad.addColorStop(1, "oklch(0.78 0.16 145 / 0)");
        ctx.fillStyle = grad;
        ctx.beginPath(); ctx.arc(x, y, 14, 0, Math.PI * 2); ctx.fill();
        // dot
        ctx.fillStyle = "oklch(0.92 0.16 145 / 0.95)";
        ctx.beginPath(); ctx.arc(x, y, 1.6, 0, Math.PI * 2); ctx.fill();
      }

      // Nodes (with breathing)
      const t = now * 0.001;
      for (const n of nodes) {
        const breath = 0.6 + Math.sin(t * 1.4 + n.phase) * 0.4;
        // halo
        const haloGrad = ctx.createRadialGradient(n.x, n.y, 0, n.x, n.y, 8);
        haloGrad.addColorStop(0, `oklch(0.78 0.16 145 / ${(0.18 * breath).toFixed(3)})`);
        haloGrad.addColorStop(1, "oklch(0.78 0.16 145 / 0)");
        ctx.fillStyle = haloGrad;
        ctx.beginPath(); ctx.arc(n.x, n.y, 8, 0, Math.PI * 2); ctx.fill();
        // core
        ctx.fillStyle = `oklch(0.85 0.14 145 / ${(0.55 + breath * 0.35).toFixed(3)})`;
        ctx.beginPath(); ctx.arc(n.x, n.y, n.r, 0, Math.PI * 2); ctx.fill();
      }

      raf = requestAnimationFrame(draw);
    }
    raf = requestAnimationFrame(draw);

    return () => {
      cancelAnimationFrame(raf);
      clearInterval(spawnTimer);
      clearInterval(accentTimer);
      ro.disconnect();
    };
  }, []);

  return <canvas ref={canvasRef} className="hero-bg-canvas" aria-hidden="true" />;
}

window.HeroBackground = HeroBackground;
