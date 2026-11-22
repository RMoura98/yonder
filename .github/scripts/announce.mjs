// Announces a GitHub release on Bluesky and Mastodon.
// Env: TAG, RELEASE_URL, DRY_RUN ("true" to print only)
//      BLUESKY_HANDLE, BLUESKY_APP_PASSWORD        (optional: skip Bluesky if missing)
//      MASTODON_INSTANCE, MASTODON_TOKEN           (optional: skip Mastodon if missing)
// Needs Node 18+ (global fetch).

const {
  TAG = "",
  RELEASE_URL = "",
  DRY_RUN = "false",
  BLUESKY_HANDLE,
  BLUESKY_APP_PASSWORD,
  MASTODON_INSTANCE,
  MASTODON_TOKEN,
} = process.env;

const dry = DRY_RUN === "true";
const version = TAG.replace(/^v/, "");
const repo = "https://github.com/RMoura98/yonder";
const url = RELEASE_URL || `${repo}/releases/tag/${TAG}`;
const posterUrl = "https://raw.githubusercontent.com/RMoura98/yonder/main/media/hero-poster.png";
const hashtags = ["OpenSource", "Rust", "Tauri", "EyeStrain"];

const intro =
  `Yonder ${version} is out — a free, open-source break reminder for Windows, macOS and Linux.\n\n` +
  `Look up. Look far. Get back to it.\n\n`;
const text = `${intro}${url}\n\n${hashtags.map((h) => "#" + h).join(" ")}`;

if (!version) throw new Error("TAG is required");

// ------------------------------------------------------------------ Bluesky
async function bluesky() {
  if (!BLUESKY_HANDLE || !BLUESKY_APP_PASSWORD) return console.log("Bluesky: no credentials, skipped");

  // Facets use UTF-8 byte offsets.
  const bytes = (s) => Buffer.byteLength(s, "utf8");
  const facets = [];
  const at = (needle) => {
    const i = text.indexOf(needle);
    return { byteStart: bytes(text.slice(0, i)), byteEnd: bytes(text.slice(0, i)) + bytes(needle) };
  };
  facets.push({ index: at(url), features: [{ $type: "app.bsky.richtext.facet#link", uri: url }] });
  for (const h of hashtags) {
    facets.push({ index: at("#" + h), features: [{ $type: "app.bsky.richtext.facet#tag", tag: h }] });
  }

  if (dry) return console.log("Bluesky (dry run):\n" + text + "\nfacets: " + JSON.stringify(facets));

  const pds = "https://bsky.social/xrpc";
  const session = await fetch(`${pds}/com.atproto.server.createSession`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ identifier: BLUESKY_HANDLE, password: BLUESKY_APP_PASSWORD }),
  }).then(ok);
  const auth = { authorization: `Bearer ${session.accessJwt}` };

  // Link card with the hero poster as its thumbnail (best effort).
  let embed;
  try {
    const img = Buffer.from(await (await fetch(posterUrl)).arrayBuffer());
    const blob = await fetch(`${pds}/com.atproto.repo.uploadBlob`, {
      method: "POST",
      headers: { ...auth, "content-type": "image/png" },
      body: img,
    }).then(ok);
    embed = {
      $type: "app.bsky.embed.external",
      external: {
        uri: url,
        title: `Yonder ${version}`,
        description: "A free, open-source break reminder for Windows, macOS and Linux.",
        thumb: blob.blob,
      },
    };
  } catch (e) {
    console.log("Bluesky: thumbnail upload failed, posting without a card:", e.message);
  }

  const res = await fetch(`${pds}/com.atproto.repo.createRecord`, {
    method: "POST",
    headers: { ...auth, "content-type": "application/json" },
    body: JSON.stringify({
      repo: session.did,
      collection: "app.bsky.feed.post",
      record: {
        $type: "app.bsky.feed.post",
        text,
        facets,
        langs: ["en"],
        createdAt: new Date().toISOString(),
        ...(embed ? { embed } : {}),
      },
    }),
  }).then(ok);
  console.log("Bluesky: posted", res.uri);
}

// ----------------------------------------------------------------- Mastodon
async function mastodon() {
  if (!MASTODON_INSTANCE || !MASTODON_TOKEN) return console.log("Mastodon: no credentials, skipped");
  if (dry) return console.log("Mastodon (dry run):\n" + text);

  const base = MASTODON_INSTANCE.replace(/\/+$/, "").replace(/^(?!https?:\/\/)/, "https://");
  const res = await fetch(`${base}/api/v1/statuses`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${MASTODON_TOKEN}`,
      "content-type": "application/json",
      "idempotency-key": `yonder-release-${TAG}`, // never double-post the same release
    },
    body: JSON.stringify({ status: text, visibility: "public", language: "en" }),
  }).then(ok);
  console.log("Mastodon: posted", res.url);
}

async function ok(r) {
  if (!r.ok) throw new Error(`${r.status} ${r.statusText}: ${await r.text()}`);
  return r.json();
}

// Run both; one failing shouldn't stop the other, but the job should still report it.
const results = await Promise.allSettled([bluesky(), mastodon()]);
const failed = results.filter((r) => r.status === "rejected");
failed.forEach((r) => console.error("Announcement failed:", r.reason.message));
if (failed.length) process.exit(1);
