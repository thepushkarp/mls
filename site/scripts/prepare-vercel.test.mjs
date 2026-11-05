import { strict as assert } from 'node:assert';
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { prepareVercel } from './prepare-vercel.mjs';

test('packages public output with discovery headers, homepage routing, and a real 404', async (t) => {
  const site = await mkdtemp(join(tmpdir(), 'site-output-'));
  t.after(() => rm(site, { recursive: true, force: true }));
  await mkdir(join(site, 'dist'));
  await writeFile(join(site, 'dist/index.html'), '<h1>Product</h1>');
  await writeFile(join(site, 'dist/404.html'), '<h1>Not found</h1>');
  await writeFile(join(site, 'dist/agents.md'), '# Installation');
  await writeFile(join(site, '.env.local'), 'PRIVATE=must-not-ship');
  await writeFile(join(site, 'vercel.json'), await readFile(new URL('../vercel.json', import.meta.url)));
  await prepareVercel(site, { orgId: 'team_test', projectId: 'prj_test' });
  const output = join(site, '.vercel/output');
  assert.deepEqual((await readdir(join(output, 'static'))).sort(), ['404.html', 'agents.md', 'index.html']);
  assert.equal(await readFile(join(output, 'static/agents.md'), 'utf8'), '# Installation');
  const { version, routes } = JSON.parse(await readFile(join(output, 'config.json'), 'utf8'));
  assert.equal(version, 3);
  const headersFor = (path) => Object.assign({}, ...routes.filter(r => r.headers && new RegExp(r.src).test(path)).map(r => r.headers));
  assert.equal(headersFor('/agents.md')['Content-Type'], 'text/markdown; charset=utf-8');
  assert.equal(headersFor('/agentsXmd')['Content-Type'], undefined);
  assert.equal(headersFor('/agents.md/extra')['Content-Type'], undefined);
  assert.match(headersFor('/')['Link'], /llms\.txt/);
  assert.equal(headersFor('/social.png')['X-Content-Type-Options'], 'nosniff');
  assert.deepEqual(routes.slice(-3), [
    { src: '^/$', dest: '/index.html' },
    { handle: 'filesystem' },
    { src: '^/.*$', dest: '/404.html', status: 404 },
  ]);
  assert.deepEqual(JSON.parse(await readFile(join(site, '.vercel/project.json'), 'utf8')), { orgId: 'team_test', projectId: 'prj_test' });
  await writeFile(join(output, 'static/stale.txt'), 'stale');
  await prepareVercel(site);
  assert(!(await readdir(join(output, 'static'))).includes('stale.txt'));
});

test('rejects an absent build before replacing deployment output', async (t) => {
  const site = await mkdtemp(join(tmpdir(), 'site-output-'));
  t.after(() => rm(site, { recursive: true, force: true }));
  await mkdir(join(site, '.vercel/output'), { recursive: true });
  await writeFile(join(site, '.vercel/output/previous'), 'preserve');
  await assert.rejects(prepareVercel(site), { code: 'ENOENT' });
  assert.equal(await readFile(join(site, '.vercel/output/previous'), 'utf8'), 'preserve');
});
