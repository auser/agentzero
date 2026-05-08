// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
	integrations: [
		starlight({
			title: 'AgentZero',
			description: 'The secure operating layer for AI agents',
			social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/auser/agentzero' }],
			sidebar: [
				{
					label: 'Getting Started',
					items: [
						{ label: 'Introduction', slug: 'getting-started/introduction' },
						{ label: 'Installation', slug: 'getting-started/installation' },
						{ label: 'Quick Start', slug: 'getting-started/quickstart' },
					],
				},
				{
					label: 'Guides',
					items: [
						{ label: 'Chat with Local Models', slug: 'guides/chat' },
						{ label: 'MCP Integration', slug: 'guides/mcp' },
						{ label: 'ACP Editor Adapter', slug: 'guides/editor-adapter' },
						{ label: 'Document Querying', slug: 'guides/document-query' },
						{ label: 'Security Scanner', slug: 'guides/scanner' },
						{ label: 'Skills & Packages', slug: 'guides/skills' },
						{ label: 'Secret Vault', slug: 'guides/vault' },
						{ label: 'Encryption at Rest', slug: 'guides/encryption' },
						{ label: 'Session History & Resume', slug: 'guides/sessions' },
						{ label: 'Audit Logs', slug: 'guides/audit' },
						{ label: 'Policy Configuration', slug: 'guides/policy' },
					],
				},
				{
					label: 'Architecture',
					items: [
						{ label: 'Overview', slug: 'architecture/overview' },
						{ label: 'Security Model', slug: 'architecture/security' },
						{ label: 'Crate Map', slug: 'architecture/crates' },
					],
				},
				{
					label: 'Reference',
					autogenerate: { directory: 'reference' },
				},
			],
		}),
	],
});
