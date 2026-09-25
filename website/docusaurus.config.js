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
          editUrl:
            'https://github.com/Nodesify/nodesify-graphify/edit/main/website/',
        },
        blog: false,
        theme: {
          customCss: require.resolve('./src/css/custom.css'),
        },
      }),
    ],
  ],
  themeConfig:
    /** @type {import('@docusaurus/preset-classic').ThemeConfig} */
    ({
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
