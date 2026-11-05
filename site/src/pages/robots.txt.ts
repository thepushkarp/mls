import data from '../content.json';
export function GET() { return new Response(`User-agent: *\nAllow: /\n\nSitemap: ${data.url}/sitemap.xml\n`); }
