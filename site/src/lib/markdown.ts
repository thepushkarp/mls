import data from '../content.json';
import agentGuide from '../agent-guide.md?raw';

const code = (value: string) => value ? `\n\n\`\`\`text\n${value}\n\`\`\`` : '';
export const overview = [
  `# ${data.name}\n\n${data.description}\n\n${data.intro}`,
  `Website: ${data.url}\nRepository: ${data.repo}`,
  `## Requirements\n\n${data.requirements.map((item) => `- ${item}`).join('\n')}`,
  `## Features\n\n${data.benefits.map((item) => `### ${item.title}\n\n${item.body}`).join('\n\n')}`,
  `## ${data.workflowTitle}\n\n${data.workflowIntro}\n\n${data.examples.map((item) => `### ${item.title}\n\n${item.body}${code(item.code)}`).join('\n\n')}`,
  `## Compatibility\n\n${data.tableIntro}\n\n| ${data.columns.join(' | ')} |\n| ${data.columns.map(() => '---').join(' | ')} |\n${data.rows.map((row) => `| ${row.join(' | ')} |`).join('\n')}`,
  `## Installation\n\n${data.installIntro}\n\n${data.installSteps.map((item, index) => `### ${index + 1}. ${item.title}\n\n${item.body}${code(item.code)}`).join('\n\n')}\n\n${data.installLinks.map((link) => `- [${link.label}](${link.url})`).join('\n')}`,
  `## Questions\n\n${data.faqs.map((item) => `### ${item.q}\n\n${item.a}`).join('\n\n')}`,
  `## Documentation\n\n${data.docs.map((link) => `- [${link.label}](${link.url})`).join('\n')}\n- [Agent guide](${data.url}/agents.md)`,
  `## Interactive preview\n\n${data.interactiveDemo}`,
  `## App screenshots\n\n${data.demoCaption}\n\n${data.screenshots.map((shot) => `- [${shot.label}](${data.url}${shot.src}): ${shot.alt}`).join('\n')}`,
  `## Artwork credits\n\n${data.artwork.map((art) => `- [${art.title}](${art.url}) by ${art.creator}, [${art.license}](${art.licenseUrl}). [Licensed source](${art.source}). Reproduced unchanged and scaled to fit.`).join('\n')}`,
].join('\n\n') + '\n';
export const agents = `# ${data.name}: installation and usage for agents\n\n${data.description}\n\n- [Website](${data.url})\n- [Discovery index](${data.url}/llms.txt)\n- [Source README](${data.repo}/blob/main/README.md)\n\n${agentGuide.trim()}\n\n## Further reading\n\n- [Product overview](${data.url}/index.md)\n${data.docs.map((link) => `- [${link.label}](${link.url})`).join('\n')}\n`;
export const llms = `# ${data.name}\n\n> ${data.description}\n\n${data.agentIntro}\n\n## Documentation\n\n- [Agent installation and usage guide](${data.url}/agents.md): Choose the tool, install and verify, run common tasks, understand results, and troubleshoot.\n- [Product overview](${data.url}/index.md): Features, compatibility, and limitations.\n- [Source repository](${data.repo}): Authoritative implementation and release instructions.\n`;
