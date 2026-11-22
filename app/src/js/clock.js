// A small watch-face badge: sun-gradient disc, 12 ticks and one sweeping hand.
// Used by the heads-up card and the cursor countdown.

let uid = 0;

/** Returns the badge markup. `size` is the rendered size in px. */
export function clockBadge(size = 44) {
  const id = `yclk${++uid}`;
  const ticks = Array.from({ length: 12 }, (_, i) => {
    const major = i % 3 === 0;
    const a = (i * 30 * Math.PI) / 180;
    const r1 = major ? 14.2 : 15.6, r2 = 17.6;
    const p = (r) => [(22 + r * Math.sin(a)).toFixed(2), (22 - r * Math.cos(a)).toFixed(2)];
    const [x1, y1] = p(r1), [x2, y2] = p(r2);
    return `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke-width="${major ? 1.9 : 1.3}" opacity="${major ? 0.95 : 0.7}"/>`;
  }).join("");
  return `<svg class="clock" width="${size}" height="${size}" viewBox="0 0 44 44" aria-hidden="true">
    <defs>
      <radialGradient id="${id}f" cx="30%" cy="24%" r="88%">
        <stop offset="0" stop-color="#FFD9A3"/>
        <stop offset=".45" stop-color="#F3A65E"/>
        <stop offset="1" stop-color="#DD6B4C"/>
      </radialGradient>
      <filter id="${id}s" x="-20%" y="-20%" width="140%" height="140%">
        <feDropShadow dx="0" dy=".6" stdDeviation=".6" flood-color="#7a2a14" flood-opacity=".45"/>
      </filter>
    </defs>
    <circle cx="22" cy="22" r="21.5" fill="url(#${id}f)"/>
    <circle cx="22" cy="22" r="21" fill="none" stroke="rgba(255,255,255,.35)" stroke-width="1"/>
    <g stroke="#fff" stroke-linecap="round" filter="url(#${id}s)">${ticks}</g>
    <g class="hand" filter="url(#${id}s)" style="transform-origin:22px 22px;transition:transform 1s linear">
      <line x1="22" y1="24.5" x2="22" y2="9.5" stroke="#fff" stroke-width="2.6" stroke-linecap="round"/>
    </g>
    <circle cx="22" cy="22" r="2.3" fill="#fff" filter="url(#${id}s)"/>
  </svg>`;
}

/** Points the hand: 0 = twelve o'clock, 1 = a full turn. */
export function setClock(root, fraction) {
  const hand = root.querySelector(".clock .hand");
  if (hand) hand.style.transform = `rotate(${(Math.max(0, Math.min(1, fraction)) * 360).toFixed(1)}deg)`;
}
