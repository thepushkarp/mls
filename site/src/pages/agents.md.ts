import { agents } from '../lib/markdown';
export function GET() { return new Response(agents, { headers: { 'Content-Type': 'text/markdown; charset=utf-8' } }); }
