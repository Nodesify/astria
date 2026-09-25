// @ts-check
// Note: type annotations allow type checking and IDE autocomplete

const lightCodeTheme = require('prism-react-renderer').themes.github;
const darkCodeTheme = require('prism-react-renderer').themes.dracula;

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'nodesify-graphify',
  tagline: 'Understand a codebase before you touch it',
  favicon: 'img/favicon.svg',
  url: 'https://nodesify.github.io',
  baseUrl: '/nodesify-graphify/',
  organizationName: 'Nodesify',
  projectName: 'nodesify-graphify',
  onBrokenLinks: 'throw',
  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },
  presets: [
    [
      'classic',
      /** @type {import('@docusaurus/preset-classic').Options} */
      ({
        docs: {
          sidebarPath: require.resolve('./sidebars.js'),
          // 0.8.0 is the latest release; website/docs tracks the next one.
          lastVersion: '0.8.0',
          versions: {
            current: {
              label: 'Next',
              banner: 'unreleased',
            },
            '0.8.0': {
              banner: 'none',
            },
          },
          editUrl: ({ versionDocsDirPath, docPath }) =>
            `https://github.com/Nodesify/nodesify-graphify/edit/main/website/${versionDocsDirPath}/${docPath}`,
        },
        blog: false,
        theme: {
          customCss: require.resolve('./src/css/custom.css'),
        },
      }),
    ],
  ],
  // Local, build-time search index — no external service. If Algolia DocSearch
  // is applied for and approved, replace this with the algolia themeConfig block.
  themes: [
    [
      require.resolve('@easyops-cn/docusaurus-search-local'),
      /** @type {import('@easyops-cn/docusaurus-search-local').PluginOptions} */
      ({
        hashed: true,
        language: ['en'],
        indexDocs: true,
        docsRouteBasePath: ['docs'],
        highlightSearchTermsOnTargetPage: true,
      }),
    ],
  ],
  themeConfig:
    /** @type {import('@docusaurus/preset-classic').ThemeConfig} */
    ({
      metadata: [
        {
          name: 'description',
          content:
            'Turn any folder into a queryable knowledge graph. Deterministic AST extraction in Rust, local embeddings, zero API keys.',
        },
        {
          property: 'og:image',
          content: 'https://nodesify.github.io/nodesify-graphify/img/og-image.png',
        },
        { property: 'og:image:width', content: '1200' },
        { property: 'og:image:height', content: '630' },
        { property: 'og:type', content: 'website' },
        { name: 'twitter:card', content: 'summary_large_image' },
        {
          name: 'twitter:image',
          content: 'https://nodesify.github.io/nodesify-graphify/img/og-image.png',
        },
      ],
      navbar: {
        title: 'nodesify-graphify',
        logo: {
          alt: 'nodesify-graphify logo',
          src: 'img/logo.svg',
        },
        items: [
          {
            type: 'docSidebar',
            sidebarId: 'docs',
            position: 'left',
            label: 'Docs',
          },
          {
            type: 'docsVersionDropdown',
            position: 'left',
          },
          {
            href: 'https://github.com/Nodesify/nodesify-graphify',
            label: 'GitHub',
            position: 'right',
          },
        ],
      },
      footer: {
        style: 'dark',
        links: [
          {
            title: 'Docs',
            items: [
              {
                label: 'Getting started',
                to: '/docs/getting-started',
              },
              {
                label: 'CLI reference',
                to: '/docs/cli',
              },
            ],
          },
          {
            title: 'Project',
            items: [
              {
                label: 'GitHub',
                href: 'https://github.com/Nodesify/nodesify-graphify',
              },
              {
                label: 'npm',
                href: 'https://www.npmjs.com/package/@nodesify/graphify',
              },
            ],
          },
        ],
        copyright: `Copyright © ${new Date().getFullYear()} Nodesify. Built with Docusaurus.`,
      },
      prism: {
        theme: lightCodeTheme,
        darkTheme: darkCodeTheme,
      },
    }),
};

module.exports = config;
