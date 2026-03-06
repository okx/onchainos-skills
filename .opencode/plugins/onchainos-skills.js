/**
 * onchainos Skills plugin for OpenCode.ai
 *
 * Injects onchainos skill context via system prompt transform.
 * Skills are discovered via OpenCode's native skill tool from the symlinked directory.
 *
 * Installation:
 *   git clone https://github.com/okx/onchainos-skills.git ~/.config/opencode/onchainos-skills
 *   ln -s ~/.config/opencode/onchainos-skills/.opencode/plugins/onchainos-skills.js ~/.config/opencode/plugins/onchainos-skills.js
 *   ln -s ~/.config/opencode/onchainos-skills/skills ~/.config/opencode/skills/onchainos-skills
 */
import path from 'path';
import fs from 'fs';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Plugin lives at .opencode/plugins/onchainos-skills.js
// Project root is two levels up: .opencode/plugins/ -> .opencode/ -> project root
const PROJECT_ROOT = path.resolve(__dirname, '../..');

// Extract YAML frontmatter and body from a SKILL.md file
const extractAndStripFrontmatter = (content) => {
  const match = content.match(/^---\n([\s\S]*?)\n---\n([\s\S]*)$/);
  if (!match) return { frontmatter: {}, content };

  const frontmatterStr = match[1];
  const body = match[2];
  const frontmatter = {};

  for (const line of frontmatterStr.split('\n')) {
    const colonIdx = line.indexOf(':');
    if (colonIdx > 0) {
      const key = line.slice(0, colonIdx).trim();
      const value = line.slice(colonIdx + 1).trim().replace(/^["']|["']$/g, '');
      frontmatter[key] = value;
    }
  }

  return { frontmatter, content: body };
};

export const OnchainOsSkillsPlugin = async ({ client, directory }) => {
  const skillsDir = path.join(PROJECT_ROOT, 'skills');
  const agentsFile = path.join(PROJECT_ROOT, 'AGENTS.md');

  const SKILL_NAMES = [
    'okx-wallet-portfolio',
    'okx-dex-market',
    'okx-dex-swap',
    'okx-dex-token',
    'okx-onchain-gateway',
  ];

  const getBootstrapContent = () => {
    const parts = [];

    // 1. Load AGENTS.md as the main bootstrap context
    if (fs.existsSync(agentsFile)) {
      const agentsContent = fs.readFileSync(agentsFile, 'utf8');
      parts.push(agentsContent.trim());
    }

    // 2. Append skill trigger conditions extracted from SKILL.md frontmatter
    const triggerLines = [];
    for (const skillName of SKILL_NAMES) {
      const skillPath = path.join(skillsDir, skillName, 'SKILL.md');
      if (!fs.existsSync(skillPath)) continue;

      const raw = fs.readFileSync(skillPath, 'utf8');
      const { frontmatter } = extractAndStripFrontmatter(raw);

      if (frontmatter.description) {
        triggerLines.push(`- **${skillName}**: ${frontmatter.description}`);
      }
    }

    if (triggerLines.length > 0) {
      parts.push(
        `## When to Load Each Skill\n\n${triggerLines.join('\n')}`
      );
    }

    // 3. Tool usage instructions for OpenCode
    parts.push(
      `## Using Skills in OpenCode

Use OpenCode's native \`skill\` tool to load a skill when triggered:
- \`skill: okx-wallet-portfolio\` — wallet balance & portfolio
- \`skill: okx-dex-market\` — token prices, charts, trade history
- \`skill: okx-dex-swap\` — DEX token swap execution
- \`skill: okx-dex-token\` — token search, rankings, holder analysis
- \`skill: okx-onchain-gateway\` — gas estimation, tx simulation, broadcasting, order tracking

**Required environment variables** (set before use):
\`\`\`
OKX_API_KEY=<your-api-key>
OKX_SECRET_KEY=<your-secret-key>
OKX_PASSPHRASE=<your-passphrase>
\`\`\`
Apply at: https://web3.okx.com/onchain-os/dev-portal`
    );

    return parts.length > 0
      ? `<ONCHAINOS_SKILLS>\n${parts.join('\n\n')}\n</ONCHAINOS_SKILLS>`
      : null;
  };

  return {
    // Inject onchainos skill context into every conversation's system prompt
    'experimental.chat.system.transform': async (_input, output) => {
      const bootstrap = getBootstrapContent();
      if (bootstrap) {
        (output.system ||= []).push(bootstrap);
      }
    },
  };
};
