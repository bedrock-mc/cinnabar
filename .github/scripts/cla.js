// CLA check for pull requests, run by .github/workflows/cla.yml from a base-branch checkout.
'use strict';
const crypto = require('crypto');

const BRANCH = 'cla-signatures';
const MARKER = '<!-- cla-check -->';
const BOT_ID = 41898282; // github-actions[bot], author of the status comment
const WEB_FLOW_ID = 19864447; // GitHub's committer for web edits and merges
const DEPENDABOT_ID = 49699333;
const AI_COAUTHOR_ID = 81847; // `claude`, from AI co-author trailers; the human author and submitter still sign
const OWNER_ID = 13991048; // HashimTheArab

module.exports = async ({ github, context, core }) => {
  const { owner, repo } = context.repo;
  const number = context.payload.pull_request?.number ?? context.payload.issue.number;
  const { data: pr } = await github.rest.pulls.get({ owner, repo, pull_number: number });
  const cla = await readCla(github, owner, repo, pr.base.ref);
  const path = `signatures/cla-v${cla.version}.json`;
  const comments = await github.paginate(github.rest.issues.listComments, { owner, repo, issue_number: number, per_page: 100 });

  let { records, sha: stored } = await readStore(github, owner, repo, path, cla.sha256);
  const seen = new Set(records.map(key));
  const fresh = [];
  for (const c of comments) {
    if (c.user?.type !== 'User') continue;
    const entity = parseSigning(c.body ?? '', cla.version);
    if (entity === false || seen.has(key({ user_id: c.user.id, entity }))) continue;
    seen.add(key({ user_id: c.user.id, entity }));
    fresh.push({
      user_id: c.user.id, login: c.user.login, ...(entity && { entity }), signed_at: c.updated_at,
      pull_request: number, comment_id: c.id, comment_url: c.html_url, statement: c.body.trim(), cla_sha256: cla.sha256,
    });
  }
  // Creating the store pins this version's digest before any signing prompt links to the text.
  if (fresh.length || !stored) {
    records = await record(github, owner, repo, path, cla.sha256, fresh, context.payload.repository.default_branch);
  }

  const { people, problems } = await contributors(github, owner, repo, pr, cla.baseSha);
  const signed = new Set(records.map((r) => r.user_id));
  const unsigned = [...people].filter(([id]) => !signed.has(id)).map(([, login]) => login);
  const ok = !unsigned.length && !problems.length;
  const stale = ok && context.eventName === 'issue_comment' && !(await rerun(github, owner, repo, pr));
  const body = ok ? doneBody(cla, stale) : statusBody(cla, unsigned, problems);
  await report(github, owner, repo, number, comments, body, !ok || stale);

  if (context.eventName === 'pull_request_target' && !ok) {
    core.setFailed(`CLA not signed by: ${[...unsigned, ...problems].join('; ')}`);
  }
};

// One acceptance per user and entity, so a later signature for an employer is kept.
const key = (r) => `${r.user_id}/${r.entity ?? ''}`;
const agreement = (version) => `I have read the Cinnabar CLA version ${version} and I agree to it`;
const ON_BEHALF = ' on behalf of ';
const statement = (version, entity) => `${agreement(version)}${entity ? ON_BEHALF + entity : ''}.`;

// Returns null for the individual sentence, the entity name for the entity sentence, or false.
function parseSigning(body, version) {
  const text = body.trim();
  if (text === statement(version)) return null;
  const prefix = agreement(version) + ON_BEHALF;
  if (!text.startsWith(prefix) || !text.endsWith('.')) return false;
  const entity = text.slice(prefix.length, -1);
  const valid = entity && entity === entity.trim() && entity.length <= 200 && !/[\r\n<>]/.test(entity);
  return valid ? entity : false;
}

async function readCla(github, owner, repo, ref) {
  const { data: branch } = await github.rest.repos.getBranch({ owner, repo, branch: ref });
  const baseSha = branch.commit.sha;
  const { data } = await github.rest.repos.getContent({ owner, repo, path: 'CLA.md', ref: baseSha });
  const text = Buffer.from(data.content, 'base64');
  const version = /^Version (\d+\.\d+)$/m.exec(text.toString('utf8'))?.[1];
  if (!version) throw new Error(`CLA.md on ${ref} has no "Version X.Y" line`);
  return {
    version,
    baseSha,
    sha256: crypto.createHash('sha256').update(text).digest('hex'),
    url: `https://github.com/${owner}/${repo}/blob/${baseSha}/CLA.md`,
  };
}

// Fails when CLA.md changed without a version bump, so no signature covers text its signer never saw.
async function readStore(github, owner, repo, path, digest) {
  let data;
  try {
    ({ data } = await github.rest.repos.getContent({ owner, repo, path, ref: BRANCH }));
  } catch (e) {
    if (e.status === 404) return { records: [], sha: undefined };
    throw e;
  }
  if (data.encoding !== 'base64') throw new Error(`${path} is too large for the contents API`);
  const store = JSON.parse(Buffer.from(data.content, 'base64').toString('utf8'));
  if (store.cla_sha256 !== digest) throw new Error(`CLA.md text differs from the text signed in ${path}; bump its version`);
  return { records: store.signatures, sha: data.sha };
}

// Appends signatures not already stored, re-reading and retrying when another run wrote first.
async function record(github, owner, repo, path, digest, fresh, defaultBranch) {
  for (let attempt = 0; attempt < 5; attempt++) {
    const store = await readStore(github, owner, repo, path, digest);
    const have = new Set(store.records.map(key));
    const add = fresh.filter((r) => !have.has(key(r)));
    if (!add.length && store.sha) return store.records;
    const records = [...store.records, ...add];
    try {
      if (!store.sha) await ensureBranch(github, owner, repo, defaultBranch);
      await github.rest.repos.createOrUpdateFileContents({
        owner, repo, path, branch: BRANCH, sha: store.sha,
        message: add.length ? `Record CLA signatures: ${add.map((r) => r.login).join(', ')}` : `Start ${path}`,
        content: Buffer.from(`${JSON.stringify({ cla_sha256: digest, signatures: records }, null, 2)}\n`).toString('base64'),
      });
      return records;
    } catch (e) {
      if (e.status !== 409 && e.status !== 422) throw e;
    }
  }
  throw new Error(`could not update ${path} on ${BRANCH} after 5 attempts`);
}

async function ensureBranch(github, owner, repo, defaultBranch) {
  try {
    await github.rest.git.getRef({ owner, repo, ref: `heads/${BRANCH}` });
    return;
  } catch (e) {
    if (e.status !== 404) throw e;
  }
  const { data: head } = await github.rest.git.getRef({ owner, repo, ref: `heads/${defaultBranch}` });
  try {
    await github.rest.git.createRef({ owner, repo, ref: `refs/heads/${BRANCH}`, sha: head.object.sha });
  } catch (e) {
    if (e.status !== 422) throw e; // created by a concurrent run
  }
}

// Returns accounts that must sign (id -> login) and identities that could not be resolved.
async function contributors(github, owner, repo, pr, baseSha) {
  const people = new Map();
  const problems = [];
  const add = (user, what) => {
    if (!user) return problems.push(what);
    if (user.id === DEPENDABOT_ID || user.id === OWNER_ID) return;
    people.set(user.id, user.login);
  };
  const quote = (s) => `\`${s.replace(/[`\r\n]/g, "'")}\``;
  const actor = (a) => quote(`${a.name} <${a.email}>`);

  add(pr.user, 'the pull request author');
  const commits = await listCommits(github, owner, repo, baseSha, pr.head.sha);
  const trailered = [];
  for (const c of commits) {
    const short = c.sha.slice(0, 7);
    add(c.author, `commit ${short} author ${actor(c.commit.author)}`);
    if (c.committer?.id !== WEB_FLOW_ID) add(c.committer, `commit ${short} committer ${actor(c.commit.committer)}`);
    const trailers = [...c.commit.message.matchAll(/^[ \t]*co-authored-by:(.*)$/gim)].map((m) => m[1].trim());
    if (trailers.length) trailered.push({ sha: c.sha, short, trailers });
  }
  const resolved = await coAuthors(github, owner, repo, trailered.map((t) => t.sha));
  for (const { sha, short, trailers } of trailered) {
    for (const t of trailers) {
      const email = /<([^<>]+)>$/.exec(t)?.[1].toLowerCase();
      const user = email && resolved.get(sha).get(email);
      if (user?.id === AI_COAUTHOR_ID) continue;
      add(user, `commit ${short} trailer ${quote(`Co-authored-by: ${t}`)}`);
    }
  }
  return { people, problems };
}

// Lists every commit in the PR; the compare API pages past the pulls API's 250-commit cap.
async function listCommits(github, owner, repo, baseSha, headSha) {
  const commits = [];
  for (let page = 1; ; page++) {
    const { data } = await github.rest.repos.compareCommitsWithBasehead({
      owner, repo, basehead: `${baseSha}...${headSha}`, per_page: 100, page,
    });
    commits.push(...data.commits);
    if (commits.length >= data.total_commits || !data.commits.length) {
      if (commits.length !== data.total_commits) throw new Error('incomplete commit list');
      return commits;
    }
  }
}

// Maps each commit to its authors' resolved accounts by lower-cased email, co-authors included.
async function coAuthors(github, owner, repo, shas) {
  const authors = (sha, after) => `object(oid: "${sha}") { ... on Commit { authors(first: 100${after ? `, after: ${JSON.stringify(after)}` : ''}) {
    pageInfo { hasNextPage endCursor } nodes { email user { databaseId login } } } } }`;
  const query = (fields) => github.graphql(
    `query($owner: String!, $repo: String!) { repository(owner: $owner, name: $repo) { ${fields} } }`, { owner, repo });
  const out = new Map();
  for (let i = 0; i < shas.length; i += 50) {
    const batch = shas.slice(i, i + 50);
    const data = await query(batch.map((sha, j) => `c${j}: ${authors(sha)}`).join('\n'));
    for (const [j, sha] of batch.entries()) {
      const map = new Map();
      let page = data.repository[`c${j}`]?.authors;
      while (page) {
        for (const n of page.nodes) {
          if (n.user && n.email) map.set(n.email.toLowerCase(), { id: n.user.databaseId, login: n.user.login });
        }
        page = page.pageInfo.hasNextPage
          ? (await query(`c: ${authors(sha, page.pageInfo.endCursor)}`)).repository.c?.authors : null;
      }
      out.set(sha, map);
    }
  }
  return out;
}

function statusBody(cla, unsigned, problems) {
  const lines = [
    MARKER,
    `Before this pull request can be merged, every contributor must agree to the [Cinnabar Contributor License Agreement](${cla.url}), version ${cla.version}.`,
  ];
  if (unsigned.length) {
    lines.push('', `Still to sign: ${unsigned.map((l) => `@${l}`).join(', ')}`, '',
      'To sign, post a comment containing exactly this sentence:', '', '```', statement(cla.version), '```', '',
      'If you contribute on behalf of an employer or other entity, post this instead, with its name:', '',
      '```', statement(cla.version, '<entity name>'), '```');
  }
  if (problems.length) {
    lines.push('', 'These identities have no linked GitHub account. Add the email to your GitHub account',
      '(Settings > Emails) or rewrite the commits, then push or comment `recheck`:', '',
      ...problems.map((p) => `- ${p}`));
  }
  return lines.join('\n');
}

const doneBody = (cla, stale) => `${MARKER}\nAll contributors have agreed to the [Cinnabar CLA](${cla.url}), version ${cla.version}.${
  stale ? '\n\nThe CLA check could not be rerun. Close and reopen this pull request to refresh it.' : ''}`;

// Updates the single status comment, creating it only when `create` is set.
async function report(github, owner, repo, number, comments, body, create) {
  const mine = comments.find((c) => c.user?.id === BOT_ID && c.body?.startsWith(MARKER));
  if (mine && mine.body !== body) {
    await github.rest.issues.updateComment({ owner, repo, comment_id: mine.id, body });
  } else if (!mine && create) {
    await github.rest.issues.createComment({ owner, repo, issue_number: number, body });
  }
}

// Reruns the latest unsuccessful PR check for the head commit; false when none can be rerun,
// such as after GitHub's 30-day rerun window.
async function rerun(github, owner, repo, pr) {
  const workflow_id = process.env.GITHUB_WORKFLOW_REF.split('@')[0].split('/').pop();
  const { data } = await github.rest.actions.listWorkflowRuns({
    owner, repo, workflow_id, event: 'pull_request_target', head_sha: pr.head.sha, per_page: 1,
  });
  const run = data.workflow_runs[0];
  if (!run) return false;
  if (run.status !== 'completed' || run.conclusion === 'success') return true;
  try {
    await github.rest.actions.reRunWorkflow({ owner, repo, run_id: run.id });
    return true;
  } catch (e) {
    if (e.status !== 403 && e.status !== 422) throw e;
    return false;
  }
}
