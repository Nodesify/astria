import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';

const features = [
  {
    title: 'Structure that survives the session',
    emoji: '🕸️',
    description: (
      <>
        Hub files, god nodes, communities, and the blast radius of any change —
        stored in SQLite and refreshed incrementally as your code changes.
      </>
    ),
  },
  {
    title: 'An honest audit trail',
    emoji: '🔍',
    description: (
      <>
        Every edge is labeled <strong>EXTRACTED / INFERRED / AMBIGUOUS</strong>{' '}
        with a numeric confidence score. You always know what was found in the
        source versus deduced, and <code>--detail high</code> filters to only
        declared facts.
      </>
    ),
  },
  {
    title: 'Answers for agents and humans',
    emoji: '🤖',
    description: (
      <>
        Query from the CLI, from any AI agent via MCP, or read the exported
        markdown wiki with plain file links — no context window required.
      </>
    ),
  },
];

function HomepageHeader() {
  const { siteConfig } = useDocusaurusContext();
  return (
    <header className={clsx('hero hero--primary', 'heroBanner')}>
      <div className="container">
        <Heading as="h1" className="hero__title">
          {siteConfig.title}
        </Heading>
        <p className="hero__subtitle">{siteConfig.tagline}</p>
        <div className="heroSubtitleNote">
          Turns any folder into a queryable knowledge graph — deterministic AST
          extraction in Rust, optional local-embedding semantics, zero API keys,
          everything on your machine.
        </div>
        <div className="buttons">
          <Link
            className="button button--secondary button--lg"
            to="/docs/getting-started">
            Get started — 2 min
          </Link>
          <Link
            className="button button--outline button--secondary button--lg buttonOutline"
            to="/docs/cli">
            CLI reference
          </Link>
        </div>
        <div className="installSnippet">
          <code>npm install -g @nodesify/graphify</code>
        </div>
      </div>
    </header>
  );
}

function Feature({ emoji, title, description }) {
  return (
    <div className={clsx('col col--4')}>
      <div className="text--center padding-horiz--md featureCard">
        <div className="featureEmoji">{emoji}</div>
        <Heading as="h3">{title}</Heading>
        <p>{description}</p>
      </div>
    </div>
  );
}

function HomepageFeatures() {
  return (
    <section className="features">
      <div className="container">
        <div className="row">
          {features.map((props, idx) => (
            <Feature key={idx} {...props} />
          ))}
        </div>
      </div>
    </section>
  );
}

function TokenBenchmark() {
  return (
    <section className="benchmark">
      <div className="container text--center">
        <Heading as="h2">73–79× fewer tokens per query</Heading>
        <p>
          Reading everything costs the whole context window. The graph answers
          in ~3,000 tokens — <strong>measured</strong>, not claimed. Every run
          prints the real comparison: actual file sizes versus actual query
          output. On tiny corpora it honestly reports less than 1×; there the
          graph's value is structure, not compression.
        </p>
      </div>
    </section>
  );
}

export default function Home() {
  const { siteConfig } = useDocusaurusContext();
  return (
    <Layout
      title={`${siteConfig.title} — queryable knowledge graphs for codebases`}
      description={siteConfig.tagline}>
      <HomepageHeader />
      <main>
        <HomepageFeatures />
        <TokenBenchmark />
      </main>
    </Layout>
  );
}
