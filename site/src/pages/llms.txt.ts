import { llms } from '../lib/markdown';
export function GET() { return new Response(llms, { headers: { 'Content-Type': 'text/markdown; charset=utf-8' } }); }
