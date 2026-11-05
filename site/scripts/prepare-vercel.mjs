import { access, cp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export async function prepareVercel(siteDirectory, link) {
  const dist = resolve(siteDirectory, 'dist');
  await access(resolve(dist, 'index.html'));
  await access(resolve(dist, '404.html'));
  const settings = JSON.parse(await readFile(resolve(siteDirectory, 'vercel.json'), 'utf8'));
  const output = resolve(siteDirectory, '.vercel/output');

  // Only generated public files belong in the deployment; source and credentials stay outside it.
  await rm(output, { recursive: true, force: true });
  await mkdir(output, { recursive: true });
  await cp(dist, resolve(output, 'static'), { recursive: true });
  await writeFile(resolve(output, 'config.json'), JSON.stringify({
    version: 3,
    routes: [
      ...settings.headers.map(({ source, headers }) => ({
        src: `^${source}$`,
        headers: Object.fromEntries(headers.map(({ key, value }) => [key, value])),
        continue: true,
      })),
      { src: '^/$', dest: '/index.html' },
      { handle: 'filesystem' },
      { src: '^/.*$', dest: '/404.html', status: 404 },
    ],
  }, null, 2) + '\n');

  // Project-scoped tokens can deploy prebuilt output without user/team settings lookups.
  if (link) {
    await writeFile(resolve(siteDirectory, '.vercel/project.json'), JSON.stringify(link) + '\n');
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const orgId = process.env.VERCEL_ORG_ID;
  const projectId = process.env.VERCEL_PROJECT_ID;
  if (Boolean(orgId) !== Boolean(projectId)) {
    throw new Error('Preparing Vercel output requires both VERCEL_ORG_ID and VERCEL_PROJECT_ID, or neither.');
  }
  await prepareVercel(fileURLToPath(new URL('..', import.meta.url)), orgId ? { orgId, projectId } : undefined);
  console.log('Prepared verified static files for Vercel.');
}
