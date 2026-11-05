import { strict as assert } from 'node:assert';
import { readFile, access } from 'node:fs/promises';
import data from '../src/content.json';

const read = (file: string) => readFile(new URL(`../dist/${file}`, import.meta.url), 'utf8');
const html = await read('index.html');
const markdown = await read('index.md');
const agents = await read('agents.md');
const llms = await read('llms.txt');
assert.equal((html.match(/<h1[ >]/g) ?? []).length, 1, 'Landing must have one h1');
assert(html.includes(`href="${data.url}/"`), 'Canonical URL missing');
for (const step of data.installSteps) assert(markdown.includes(step.body), 'Markdown installation content missing');
const guide = await readFile(new URL('../src/agent-guide.md', import.meta.url), 'utf8');
assert(agents.includes(guide.trim()), 'Published agent guide is incomplete');
assert(llms.includes(`${data.url}/agents.md`), 'Agent guide not discoverable');
assert((await read('sitemap.xml')).includes(data.url), 'Sitemap host mismatch');
const ids = new Set([...html.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]));
for (const [, hash] of html.matchAll(/href="#([^"]+)"/g)) assert(ids.has(hash), `Broken anchor #${hash}`);
for (const [, path] of html.matchAll(/(?:src|href)="(\/[^"#?]*)"/g)) {
  if (path === '/') continue;
  await access(new URL(`../dist${path}`, import.meta.url));
}
for (const shot of data.screenshots) {
  await access(new URL(`../dist${shot.darkSrc}`, import.meta.url));
  await access(new URL(`../dist${shot.src}`, import.meta.url));
  assert(markdown.includes(shot.alt), `Screenshot description missing: ${shot.label}`);
}
const social = await readFile(new URL('../dist/social.png', import.meta.url));
assert.equal(social.subarray(1, 4).toString(), 'PNG', 'Social image must be a PNG');
assert.equal(social.readUInt32BE(16), 1200, 'Social image width must match OG metadata');
assert.equal(social.readUInt32BE(20), 630, 'Social image height must match OG metadata');
assert(html.includes(`<title>${data.seoTitle}</title>`), 'Descriptive page title missing');
for (const attribute of ['property="og:image:alt"', 'name="twitter:image:alt"']) {
  assert(html.includes(`${attribute} content="${data.socialImageAlt}"`), 'Social image description missing');
}
const schema = JSON.parse(html.match(/<script[^>]*type="application\/ld\+json"[^>]*>(.*?)<\/script>/s)?.[1] ?? 'null');
assert.equal(schema?.operatingSystem, data.operatingSystem, 'Software platform missing');
assert.equal(schema?.author?.name, 'Pushkar Patel', 'Software author missing');
assert.equal(schema?.image, `${data.url}/social.png`, 'Software image missing');
assert.deepEqual(schema?.screenshot, data.screenshots.map(shot => data.url + shot.src), 'Product screenshots missing');
const robots = process.env.VERCEL_ENV === 'preview' ? 'noindex, follow' : 'index, follow, max-image-preview:large';
assert(html.includes(`name="robots" content="${robots}"`), 'Deployment indexing policy mismatch');
console.log(`${data.shortName}: static routes, content, discovery, anchors, and assets verified.`);
