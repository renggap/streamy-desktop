// Streamy Desktop update proxy.
// Holds the GitHub PAT server-side so the private repo's release feed
// is readable by the app without embedding any secret in the binary.

const REPO = "renggap/streamy-desktop";
const API = `https://api.github.com/repos/${REPO}/releases/latest`;

// ponytail: single shared endpoint; add a bearer-token check here if the
// worker URL ever leaks and gets hammered by third parties.
export default {
  async fetch(request, env) {
    if (request.method !== "GET") {
      return new Response("method not allowed", { status: 405 });
    }
    const url = new URL(request.url);
    if (url.pathname !== "/latest") {
      return new Response("not found", { status: 404 });
    }
    const upstream = await fetch(API, {
      headers: {
        Authorization: `Bearer ${env.GITHUB_TOKEN}`,
        Accept: "application/vnd.github+json",
        "User-Agent": "streamy-update-proxy",
        // Cache the GitHub response at the edge for 5 min; the app checks
        // once per launch, so this keeps us far under rate limits.
        "If-None-Match": request.headers.get("if-none-match") ?? "",
      },
    });
    if (upstream.status === 404) {
      // No releases published yet — report "up to date" rather than error.
      return Response.json({ version: "0.0.0", notes: null, url: `https://github.com/${REPO}/releases` });
    }
    if (!upstream.ok) {
      return new Response("upstream error", { status: 502 });
    }
    const rel = await upstream.json();
    return Response.json(
      {
        version: rel.tag_name.replace(/^v/, ""),
        notes: (rel.body ?? "").split(/\n\s*\n/)[0]?.trim() || null,
        url: rel.html_url,
      },
      { headers: { "Cache-Control": "public, max-age=300" } },
    );
  },
};
