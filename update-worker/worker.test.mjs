// Exercise worker logic without deploy: import the fetch handler with a
// mock env, hit every branch.
import worker from '/home/streamy-desktop/update-worker/worker.js';

const ok = (name, cond) => { console.log((cond ? 'PASS' : 'FAIL'), name); if (!cond) process.exitCode = 1; };

// mock global fetch: GitHub API responses
const realFetch = globalThis.fetch;
const makeReq = (path, init) => new Request('https://proxy.test' + path, init);

// 1. GET /latest with a release
globalThis.fetch = async () => new Response(JSON.stringify({
  tag_name: 'v1.6.0', html_url: 'https://github.com/x/releases/tag/v1.6.0',
  body: 'Notes line 1\nline 2\n\nSecond para',
}), { status: 200 });
let r = await worker.fetch(makeReq('/latest'), { GITHUB_TOKEN: 't' });
let j = await r.json();
ok('happy path version', j.version === '1.6.0');
ok('notes first paragraph only', j.notes === 'Notes line 1\nline 2');
ok('url passthrough', j.url.endsWith('/v1.6.0'));

// 2. GitHub 404 (no releases)
globalThis.fetch = async () => new Response('{"message":"Not Found"}', { status: 404 });
r = await worker.fetch(makeReq('/latest'), { GITHUB_TOKEN: 't' });
j = await r.json();
ok('no-release sentinel 0.0.0', j.version === '0.0.0');

// 3. upstream 500 -> 502
globalThis.fetch = async () => new Response('boom', { status: 500 });
r = await worker.fetch(makeReq('/latest'), { GITHUB_TOKEN: 't' });
ok('upstream error -> 502', r.status === 502);

// 4. wrong method
r = await worker.fetch(makeReq('/latest', { method: 'POST' }), {});
ok('POST -> 405', r.status === 405);

// 5. wrong path
r = await worker.fetch(makeReq('/nope'), {});
ok('bad path -> 404', r.status === 404);

// 6. v-prefix strip + empty body notes
globalThis.fetch = async () => new Response(JSON.stringify({
  tag_name: 'v2.0.0', html_url: 'u', body: '',
}), { status: 200 });
r = await worker.fetch(makeReq('/latest'), { GITHUB_TOKEN: 't' });
j = await r.json();
ok('empty body -> notes null', j.notes === null && j.version === '2.0.0');

globalThis.fetch = realFetch;
