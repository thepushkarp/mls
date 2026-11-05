import { overview } from '../lib/markdown';
export function GET() { return new Response(overview, { headers: { 'Content-Type': 'text/markdown; charset=utf-8' } }); }
