// A real riki over a throwaway content repo: tempdir `file://` upstream, tempdir cache-dir, its
// own config, a free loopback port. Git runs with no global or system config, so the host's
// hooks, signing and URL rewrites never reach the test repos.

import { execFileSync, spawn } from 'node:child_process'
import type { ChildProcess } from 'node:child_process'
import fs from 'node:fs'
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'
import { test as base } from '@playwright/test'

const BIN = process.env['RIKI_BIN'] ?? path.join(import.meta.dirname, '..', '..', 'target', 'debug', 'riki')

const GIT_ENV = {
  ...process.env,
  GIT_CONFIG_GLOBAL: '/dev/null',
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_NAME: 'Seed',
  GIT_AUTHOR_EMAIL: 'seed@example.test',
  GIT_COMMITTER_NAME: 'Seed',
  GIT_COMMITTER_EMAIL: 'seed@example.test',
}

function git(args: string[], cwd?: string): string {
  return execFileSync('git', args, { cwd, env: GIT_ENV, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] })
}

async function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = net.createServer()
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const address = server.address()
      server.close(() => (typeof address === 'object' && address ? resolve(address.port) : reject(new Error('no port'))))
    })
  })
}

export class Riki {
  readonly dir: string
  readonly upstream: string
  readonly work: string
  url = ''
  #child: ChildProcess | null = null
  #log = ''

  constructor() {
    this.dir = fs.mkdtempSync(path.join(os.tmpdir(), 'riki-e2e-'))
    this.upstream = path.join(this.dir, 'upstream.git')
    this.work = path.join(this.dir, 'work')
    git(['init', '--bare', '--initial-branch=main', this.upstream])
    git(['clone', '--quiet', this.upstream, this.work])
    git(['checkout', '--quiet', '-b', 'main'], this.work)
  }

  /** Commit `files` to upstream main, the way a laptop push would. */
  push(files: Record<string, string>, message = 'seed'): void {
    if (git(['ls-remote', '--heads', 'origin', 'main'], this.work).trim()) {
      git(['pull', '--quiet', '--ff-only', 'origin', 'main'], this.work)
    }
    for (const [file, text] of Object.entries(files)) {
      fs.mkdirSync(path.dirname(path.join(this.work, file)), { recursive: true })
      fs.writeFileSync(path.join(this.work, file), text)
    }
    git(['add', '-A'], this.work)
    git(['commit', '--quiet', '--no-gpg-sign', '-m', message], this.work)
    git(['push', '--quiet', 'origin', 'HEAD:main'], this.work)
  }

  /** `push`, then wait for the running riki's poller to serve `url` (a 200). */
  async pushAndServe(files: Record<string, string>, url: string): Promise<void> {
    this.push(files)
    const deadline = Date.now() + 15_000
    while (Date.now() < deadline) {
      if ((await fetch(`${this.url}${url}`)).ok) return
      await new Promise((r) => setTimeout(r, 200))
    }
    throw new Error(`riki did not serve ${url} within 15s`)
  }

  /** The file as upstream holds it now. */
  file(file: string): string {
    return git(['--git-dir', this.upstream, 'show', `main:${file}`])
  }

  /** How many commits upstream's main holds. */
  commitCount(): number {
    return Number(git(['--git-dir', this.upstream, 'rev-list', '--count', 'main']).trim())
  }

  /** Author email of upstream's newest commit. */
  lastAuthor(): string {
    return git(['--git-dir', this.upstream, 'log', '-1', '--format=%ae', 'main']).trim()
  }

  async start(): Promise<void> {
    if (!fs.existsSync(BIN)) throw new Error(`riki binary not found at ${BIN}: run \`cargo build -p riki-server\` first`)
    const port = await freePort()
    const config = path.join(this.dir, 'riki.yml')
    fs.writeFileSync(
      config,
      [
        `listen: 127.0.0.1:${port}`,
        'content:',
        `  remote: file://${this.upstream}`,
        '  branch: main',
        `  cache-dir: ${path.join(this.dir, 'cache', 'content.git')}`,
        'git:',
        '  timeout: 10s',
        '  poll-interval: 1s',
        '',
      ].join('\n'),
    )
    const child = spawn(BIN, ['--config', config], { env: GIT_ENV, stdio: ['ignore', 'pipe', 'pipe'] })
    child.stdout?.on('data', (chunk: Buffer) => (this.#log += chunk.toString()))
    child.stderr?.on('data', (chunk: Buffer) => (this.#log += chunk.toString()))
    this.#child = child
    this.url = `http://127.0.0.1:${port}`
    const deadline = Date.now() + 15_000
    while (Date.now() < deadline) {
      if (child.exitCode !== null) throw new Error(`riki exited ${child.exitCode}:\n${this.#log}`)
      const ready = await fetch(`${this.url}/ready`).then((r) => r.ok, () => false)
      if (ready) return
      await new Promise((r) => setTimeout(r, 100))
    }
    throw new Error(`riki did not become ready:\n${this.#log}`)
  }

  get log(): string {
    return this.#log
  }

  stop(): void {
    this.#child?.kill('SIGTERM')
    fs.rmSync(this.dir, { recursive: true, force: true })
  }
}

/** `riki`: a started server seeded with a home page and a canonical guide page. */
export const test = base.extend<{ riki: Riki }>({
  riki: async ({}, use, testInfo) => {
    const riki = new Riki()
    riki.push({
      'README.md': '# Home\n\nWelcome.\n',
      'guide.md': '# Guide\n\nA canonical page.\n\n- one\n- two\n',
      'starred.md': '# Starred\n\n* a star bullet the editor rewrites\n',
    })
    await riki.start()
    await use(riki)
    if (testInfo.status !== testInfo.expectedStatus) console.log(riki.log)
    riki.stop()
  },
})

export { expect } from '@playwright/test'
