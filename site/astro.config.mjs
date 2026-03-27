import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import starlightClientMermaid from "@pasqal-io/starlight-client-mermaid";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  site: "https://tryagentzero.com",
  base: "/",
  outDir: "../gh-pages",
  integrations: [
    starlight({
      plugins: [starlightClientMermaid()],
      title: "AgentZero",
      social: [
        { icon: "github", label: "GitHub", href: "https://github.com/auser/agentzero" },
      ],
      components: {
        Header: "./src/components/Header.astro",
        ThemeProvider: "./src/components/ThemeProvider.astro",
        ThemeSelect: "./src/components/ThemeSelect.astro",
      },
      customCss: [
        "./src/styles/global.css",
        "./src/styles/landing.css",
      ],
      sidebar: [
        {
          label: "Getting Started",
          items: [
            { label: "Overview", slug: "overview" },
            { label: "Installation", slug: "installation" },
            { label: "Quick Start", slug: "quickstart" },
            { label: "Your First Hour", slug: "guides/first-hour" },
          ],
        },
        {
          label: "Guides",
          items: [
            { label: "Client SDKs", slug: "guides/sdks" },
            { label: "Provider Setup", slug: "guides/providers" },
            { label: "Gateway Deployment", slug: "guides/deployment" },
            { label: "Production Setup", slug: "guides/production" },
            { label: "MCP Servers", slug: "guides/mcp" },
            { label: "Channels", slug: "guides/channels" },
            { label: "Multi-Agent Patterns", slug: "guides/multi-agent" },
            { label: "Autopilot Mode", slug: "guides/autopilot" },
            { label: "Daily Usage", slug: "guides/daily-usage" },
            { label: "Testing", slug: "guides/testing" },
            { label: "Plugin Authoring", slug: "guides/plugins" },
            { label: "FFI Bindings (deprecated)", slug: "guides/ffi-bindings" },
            { label: "Android", slug: "guides/android" },
            { label: "Raspberry Pi", slug: "guides/raspberry-pi" },
          ],
        },
        {
          label: "Configuration",
          items: [
            { label: "Config Reference", slug: "config/reference" },
            { label: "Environment Variables", slug: "config/environment" },
          ],
        },
        {
          label: "Architecture",
          items: [
            { label: "System Overview", slug: "architecture" },
            { label: "Trait System", slug: "architecture/traits" },
            { label: "Roadmap", slug: "roadmap" },
          ],
        },
        {
          label: "Reference",
          items: [
            { label: "CLI Commands", slug: "reference/commands" },
            { label: "Gateway API", slug: "reference/gateway" },
            { label: "Tools & Plugins", slug: "reference/tools" },
            { label: "Plugin API", slug: "reference/plugin-api" },
            { label: "Benchmarks", slug: "reference/benchmarks" },
            { label: "Release Process", slug: "reference/release" },
          ],
        },
        {
          label: "Architecture Decisions",
          autogenerate: { directory: "adr" },
        },
        {
          label: "Security",
          items: [
            { label: "Security Overview", slug: "security/overview" },
            { label: "Security Boundaries", slug: "security/boundaries" },
            { label: "Threat Model", slug: "security/threat-model" },
            { label: "Dependency Policy", slug: "security/dependency-policy" },
          ],
        },
      ],
    }),
  ],
  vite: {
    plugins: [tailwindcss()],
  },
});
