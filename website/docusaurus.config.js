// @ts-check
// Note: type annotations allow type checking and IDE autocomplete

const lightCodeTheme = require('prism-react-renderer').themes.github;
const darkCodeTheme = require('prism-react-renderer').themes.dracula;

// The shipped product version (the npm package the docs describe), not the
// website's own version. Used by the landing-page badge.
const productVersion = require('../packages/astria-cli/package.json').version;

// Old (pre-reorganization) doc paths → their new folder. Wired as a
// `createRedirects` callback so it works for the Next version now and keeps
// working for any future version served at the root prefix after a version
// cut; versioned_docs keep the old paths and match nothing, so no conflicts.
const movedDocs = {
  guides: ['wiki-and-exports', 'semantic-enrichment'],
  reference: ['cli', 'language-support'],
  explanation: ['architecture', 'benchmarks'],
};

function redirectsForMovedDocs(existingPath) {
  for (const [folder, names] of Object.entries(movedDocs)) {
    for (const name of names) {
      const marker = `/${folder}/${name}`;
      if (existingPath.includes(marker)) {
        return [existingPath.replace(marker, `/${name}`)];
      }
    }
  }
  return undefined;
}

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'astria',
  tagline: 'Understand a codebase before you touch it',
  favicon: 'img/favicon.svg',
  url: 'https://nodesify.github.io',
  baseUrl: '/astria/',
  organizationName: 'Nodesify',
  projectName: 'astria',
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
          // 1.0.0 is the latest release; website/docs tracks the next one.
          lastVersion: '1.0.0',
          versions: {
            current: {
              label: 'Next',
              banner: 'unreleased',
            },
            '1.0.0': {
              banner: 'none',
            },
            '0.8.0': {
              banner: 'none',
            },
          },
          // website/ currently lives on the develop branch; switch back to
          // main once it is merged there, or edit links will 404.
          editUrl: ({ versionDocsDirPath, docPath }) =>
            `https://github.com/Nodesify/astria/edit/develop/website/${versionDocsDirPath}/${docPath}`,
        },
        blog: {
          showReadingTime: true,
          blogSidebarTitle: 'Release notes',
          blogSidebarCount: 'ALL',
        },
        theme: {
          customCss: require.resolve('./src/css/custom.css'),
        },
      }),
    ],
  ],
  markdown: {
    mermaid: true,
  },
  themes: [
    '@docusaurus/theme-mermaid',
    [
      require.resolve('@easyops-cn/docusaurus-search-local'),
      /** @type {import('@easyops-cn/docusaurus-search-local').PluginOptions} */
      ({
        hashed: true,
        language: ['en'],
        indexDocs: true,
        indexBlog: true,
        docsRouteBasePath: ['docs'],
        highlightSearchTermsOnTargetPage: true,
      }),
    ],
  ],
  plugins: [
    [
      '@docusaurus/plugin-client-redirects',
      /** @type {import('@docusaurus/plugin-client-redirects').PluginOptions} */
      ({
        createRedirects: redirectsForMovedDocs,
      }),
    ],
    'docusaurus-plugin-llms',
  ],
  customFields: {
    productVersion,
  },
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
          content: 'https://nodesify.github.io/astria/img/og-image.png',
        },
        { property: 'og:image:width', content: '1200' },
        { property: 'og:image:height', content: '630' },
        { property: 'og:type', content: 'website' },
        { name: 'twitter:card', content: 'summary_large_image' },
        {
          name: 'twitter:image',
          content: 'https://nodesify.github.io/astria/img/og-image.png',
        },
      ],
      navbar: {
        title: 'astria',
        logo: {
          alt: 'astria logo',
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
            to: '/blog',
            label: 'Blog',
            position: 'left',
          },
          {
            href: 'https://github.com/Nodesify/astria',
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
                to: '/docs/reference/cli',
              },
            ],
          },
          {
            title: 'Project',
            items: [
              {
                label: 'GitHub',
                href: 'https://github.com/Nodesify/astria',
              },
              {
                label: 'npm',
                href: 'https://www.npmjs.com/package/@nodesify/astria',
              },
              {
                label: 'Website',
                href: 'https://nodesify.com',
              },
              {
                label: 'Release notes',
                to: '/blog',
              },
            ],
          },
        ],
        copyright: `Copyright © ${new Date().getFullYear()} <a href="https://nodesify.com">Nodesify</a>. Built with Docusaurus.`,
      },
      prism: {
        theme: lightCodeTheme,
        darkTheme: darkCodeTheme,
      },
    }),
};

module.exports = config;
