import { defineConfig } from 'vitepress'

// https://vitepress.dev/reference/site-config
export default defineConfig({
  title: "TrackMaker-rs",
  description: "A high-performance audio-based information transmission tool, written in Rust",
  base: "/TrackMaker-rs/",
  themeConfig: {
    // https://vitepress.dev/reference/default-theme-config
    nav: [
      { text: 'Home', link: '/' },
      { text: 'Examples', link: '/markdown-examples' },
      { text: "Projects", link: "/proj/proj1" },
    ],

    logo: '/logo.png',

    sidebar: [
      {
        text: 'Examples',
        items: [
          { text: 'Markdown Examples', link: '/markdown-examples' },
          { text: 'Runtime API Examples', link: '/api-examples' }
        ]
      },
      {
        text: 'Projects',
        items: [
          { text: 'Project 1', link: '/proj/proj1' }
        ]
      }
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/HeZeBang/TrackMaker-rs' }
    ],

    editLink: {
      pattern: 'https://github.com/HeZeBang/TrackMaker-rs/edit/main/docs/:path',
      text: 'Edit this page on GitHub'
    },

    footer: {
      message: 'Released under the MIT License.',
      copyright: 'Made with ❤️ by HeZeBang & WilliamZhang'
    }
  },
  markdown: {
    math: true,
  }
})
