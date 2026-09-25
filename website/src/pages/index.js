import React from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import styles from './index.module.css';

const Stats = [
  { value: '50–110×', label: 'fewer tokens per query, measured' },
  { value: '21', label: 'languages via tree-sitter' },
  { value: '0', label: 'API keys required' },
  { value: '100%', label: 'local — your code never leaves' },
];

const features = [
  {
    title: 'Deterministic AST extraction',
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <path d="M5 6 19 6M5 6 12 18M19 6 12 18" />
        <circle cx="5" cy="6" r="2.5" /><circle cx="19" cy="6" r="2.5" /><circle cx="12" cy="18" r="2.5" />
      </svg>
    ),
    description: (
      <>
        Tree-sitter grammars in a native Rust core — no regex guessing. Every
        node is anchored at <code>file:line</code> and every edge carries its
        provenance.
      </>
    ),
  },
  {
    title: 'Structure that survives the session',
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <ellipse cx="12" cy="5" rx="8" ry="2.6" /><path d="M4 5v7c0 1.4 3.6 2.6 8 2.6s8-1.2 8-2.6V5" /><path d="M4 12v7c0 1.4 3.6 2.6 8 2.6s8-1.2 8-2.6v-7" />
      </svg>
    ),
    description: (
      <>
        Hub files, god nodes, communities, and blast radius stored in SQLite —
        rebuilt incrementally in seconds as your code changes.
      </>
    ),
  },
  {
    title: 'An honest audit trail',
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <path d="M12 3l7 3v5c0 5-3.5 8-7 9-3.5-1-7-4-7-9V6l7-3z" /><path d="M9 12l2 2 4-4.5" />
      </svg>
    ),
    description: (
      <>
        Every edge is labeled <code>EXTRACTED</code>, <code>INFERRED</code> or{' '}
        <code>AMBIGUOUS</code> with a numeric confidence score.{' '}
        <code>--detail high</code> shows declared facts only.
      </>
    ),
  },
  {
    title: 'Answers for agents and humans',
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <rect x="5" y="9" width="14" height="11" rx="2.5" /><path d="M12 9V5" /><circle cx="12" cy="4" r="1" /><circle cx="9.5" cy="14" r="1.1" /><circle cx="14.5" cy="14" r="1.1" />
      </svg>
    ),
    description: (
      <>
        Query from the CLI, point any MCP-capable agent at the built-in stdio
        server, or ask for an Aider-style repo map ranked by PageRank.
      </>
    ),
  },
  {
    title: 'Wiki & Obsidian export',
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <path d="M12 6c-2-1.6-4.9-2.2-8-2.2v14.4c3.1 0 6 .6 8 2.2 2-1.6 4.9-2.2 8-2.2V3.8c-3.1 0-6 .6-8 2.2z" /><path d="M12 6v14.4" />
      </svg>
    ),
    description: (
      <>
        An agent-crawlable markdown wiki, or a full Obsidian vault with{' '}
        <code>[[wikilinks]]</code> and a canvas of your communities. Regenerated
        on every update.
      </>
    ),
  },
  {
    title: 'Local semantic layer',
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <rect x="7" y="7" width="10" height="10" rx="2" /><path d="M9 3v4M15 3v4M9 17v4M15 17v4M3 9h4M3 15h4M17 9h4M17 15h4" />
      </svg>
    ),
    description: (
      <>
        Optional local embeddings add <code>similar_to</code> edges and semantic
        query recall — no API key, offline after a one-time download. Or bring
        your own LLM.
      </>
    ),
  },
];

const pipeline = ['detect', 'extract', 'build', 'cluster', 'analyze', 'report'];

function HeroTerminal() {
  return (
    <div className={styles.terminal}>
      <div className={styles.terminalBar}>
        <span className={clsx(styles.dot, styles.dotRed)} />
        <span className={clsx(styles.dot, styles.dotYellow)} />
        <span className={clsx(styles.dot, styles.dotGreen)} />
        <span className={styles.terminalTitle}>agent — zsh</span>
      </div>
      <div className={styles.terminalBody}>
        <div><span className={styles.tPrompt}>$</span> <span className={styles.tCmd}>astria query</span> <span className={styles.tArg}>"where does auth live?"</span></div>
        <div className={styles.tOut}>
          <div><span className={styles.tNode}>NODE</span>&nbsp;&nbsp;authenticate_user()&nbsp;&nbsp;<span className={styles.tLoc}>src/auth/auth.rs:45</span></div>
          <div><span className={styles.tNode}>NODE</span>&nbsp;&nbsp;AuthMiddleware&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;<span className={styles.tLoc}>src/auth/middleware.rs:12</span></div>
          <div><span className={styles.tEdge}>EDGE</span>&nbsp;&nbsp;AuthMiddleware <span className={styles.tRel}>─CALLS→</span> authenticate_user&nbsp;&nbsp;<span className={styles.tConf}>EXTRACTED · 0.97</span> <span className={styles.tLoc}>@middleware.rs:28</span></div>
        </div>
        <div className={styles.tMeta}>~3,000 tokens · 73× fewer than reading the corpus · graph built 2m ago</div>
      </div>
    </div>
  );
}

function Hero() {
  const { siteConfig } = useDocusaurusContext();
  const version = siteConfig.customFields?.productVersion ?? '';
  return (
    <header className={styles.hero}>
      <div className={clsx('container', styles.heroInner)}>
        <div className={styles.heroCopy}>
          <div className={styles.badge}>v{version} — markdown wiki + Obsidian vault export</div>
          <Heading as="h1" className={styles.heroTitle}>
            Understand a codebase <span className={styles.heroAccent}>before you touch it</span>
          </Heading>
          <p className={styles.heroSubtitle}>
            <strong>{siteConfig.title}</strong> turns any folder into a queryable knowledge
            graph — deterministic AST extraction in Rust, optional local-embedding
            semantics, zero API keys, everything on your machine.
          </p>
          <div className={styles.heroActions}>
            <Link className={clsx('button', 'button--lg', styles.primaryBtn)} to="/docs/getting-started">
              Get started
            </Link>
            <Link className={clsx('button', 'button--lg', styles.ghostBtn)} href="https://github.com/Nodesify/astria">
              GitHub ↗
            </Link>
          </div>
          <div className={styles.installRow}>
            <code className={styles.installCmd}>npm install -g @nodesify/astria</code>
          </div>
        </div>
        <div className={styles.heroVisual}>
          <HeroTerminal />
        </div>
      </div>
    </header>
  );
}

function StatsBand() {
  return (
    <section className={styles.statsBand}>
      <div className="container">
        <div className={styles.statsGrid}>
          {Stats.map((s) => (
            <div key={s.value} className={styles.stat}>
              <div className={styles.statValue}>{s.value}</div>
              <div className={styles.statLabel}>{s.label}</div>
            </div>
          ))}
        </div>
        <div className={styles.statsFoot}>
          <Link to="/docs/benchmarks">
            The numbers, the methodology, and a head-to-head vs the original Graphify →
          </Link>
        </div>
      </div>
    </section>
  );
}

function Features() {
  return (
    <section className={styles.features}>
      <div className="container">
        <Heading as="h2" className={styles.sectionTitle}>Why a graph, not a folder</Heading>
        <p className={styles.sectionLead}>
          Three things a folder full of files can't give you — plus the exports and
          integrations that make the graph part of your daily loop.
        </p>
        <div className={styles.featureGrid}>
          {features.map((f) => (
            <div key={f.title} className={styles.featureCard}>
              <div className={styles.featureIcon}>{f.icon}</div>
              <Heading as="h3" className={styles.featureTitle}>{f.title}</Heading>
              <p className={styles.featureDesc}>{f.description}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

function Pipeline() {
  return (
    <section className={styles.pipeline}>
      <div className="container text--center">
        <Heading as="h2" className={styles.sectionTitle}>One pipeline, pure stages</Heading>
        <p className={styles.sectionLead}>
          Each stage is a pure function in its own Rust crate. SQLite persists; petgraph computes.
        </p>
        <div className={styles.pipelineRow}>
          {pipeline.map((stage, i) => (
            <React.Fragment key={stage}>
              {i > 0 && <span className={styles.pipelineArrow}>→</span>}
              <span className={styles.pipelineStage}>{stage}()</span>
            </React.Fragment>
          ))}
        </div>
      </div>
    </section>
  );
}

function CtaBand() {
  return (
    <section className={styles.ctaBand}>
      <div className="container text--center">
        <Heading as="h2" className={styles.ctaTitle}>Put your codebase on the graph</Heading>
        <p className={styles.ctaLead}>
          One command, one local directory, no cloud. See{' '}
          <a href="https://github.com/Nodesify/astria/tree/main/worked" target="_blank" rel="noopener noreferrer">
            worked examples
          </a>{' '}
          — including what the graph got wrong.
        </p>
        <div className={styles.heroActions}>
          <Link className={clsx('button', 'button--lg', styles.primaryBtn)} to="/docs/getting-started">
            Read the docs
          </Link>
          <Link className={clsx('button button--lg', styles.ghostBtn)} to="/docs/cli">
            CLI reference
          </Link>
        </div>
      </div>
    </section>
  );
}

export default function Home() {
  const { siteConfig } = useDocusaurusContext();
  return (
    <Layout
      title={`${siteConfig.title} — queryable knowledge graphs for codebases`}
      description="Turn any folder into a queryable knowledge graph. Deterministic AST extraction in Rust, local embeddings, zero API keys.">
      <Hero />
      <main>
        <StatsBand />
        <Features />
        <Pipeline />
        <CtaBand />
      </main>
    </Layout>
  );
}
