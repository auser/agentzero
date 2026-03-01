import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  site: "https://agentzero.github.io",
  base: "/agentzero/",
  outDir: "../gh-pages",
  integrations: [
    starlight({
      title: "AgentZero",
      social: [
        { icon: "github", label: "GitHub", href: "https://github.com/auser/agentzero" },
      ],
      customCss: [
        "./src/styles/global.css",
        "./src/styles/landing.css",
      ],
      sidebar: [
        {
          label: "Getting Started",
          items: [
            { label: "Overview", slug: "overview" },
            { label: "Architecture", slug: "architecture" },
            { label: "Roadmap", slug: "roadmap" },
          ],
        },
        {
          label: "Reference",
          items: [
            { label: "Command Reference", slug: "reference/commands" },
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
            { label: "Threat Model", slug: "security/threat-model" },
            {
              label: "Dependency Policy",
              slug: "security/dependency-policy",
            },
          ],
        },
      ],
    }),
  ],
  vite: {
    plugins: [tailwindcss()],
  },
});
