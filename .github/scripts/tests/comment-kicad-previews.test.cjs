const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const publish = require('../comment-kicad-previews.cjs');

const image = '1234567890abcdef1234-after-abcdef123456.png';
const manifest = () => ({
  base: 'a'.repeat(40), head: 'b'.repeat(40), entries: [{
    source: 'pcb/main/main.kicad_sch', status: 'added', before: false, after: true,
    images: { before: {}, after: { 'sheet: main': image } }, errors: {},
  }],
});

test('new schematic has after image and a separate marker from ERC/DRC', () => {
  const body = publish.buildBody(manifest(), 'https://images/sha', 'https://run', 'https://artifact', 1);
  assert.match(body, /homecockpit-kicad-previews/);
  assert.doesNotMatch(body, /homecockpit-kicad-ci/);
  assert.match(body, /新規追加/);
  assert.match(body, /!\[after\]\(https:\/\/images\/sha\//);
});

test('before and after, deletion, missing pages and failed exports are explicit', () => {
  const data = manifest();
  const entry = data.entries[0];
  entry.before = true;
  entry.status = 'modified';
  entry.images.before.front = image.replace('-after-', '-before-');
  const body = publish.buildBody(data, 'https://images', 'https://run', 'https://artifact', 1);
  assert.match(body, /!\[before\]/);
  assert.match(body, /該当ページなし/);
  entry.after = false;
  entry.images.after = {};
  entry.status = 'deleted';
  assert.match(publish.buildBody(data, '', '', '', 1), /削除済み/);
  entry.after = true;
  entry.errors.after = 'export failed';
  assert.match(publish.buildBody(data, '', '', '', 1), /画像生成に失敗/);
});

test('paths are escaped and large comments stay within GitHub limits', () => {
  const data = manifest();
  data.entries[0].source = '<img>|`\n';
  const body = publish.buildBody(data, '', '', '', 1);
  assert.doesNotMatch(body, /<img>/);
  data.entries = Array.from({ length: 1000 }, () => data.entries[0]);
  const large = publish.buildBody(data, 'https://images', 'https://run', 'https://artifact', 1);
  assert.ok(large.length < 60000);
  assert.match(large, /残りの画像/);
});

async function withMock(data, {
  comments = [], currentHead = data.head, headAfterUpload = currentHead,
  branchExists = false, runOverrides = {}, prOverrides = {},
  artifacts = [{ name: 'kicad-previews', id: 3 }],
  imageContent = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j1ioAAAAASUVORK5CYII=', 'base64'),
  prepareDirectory, expectedError,
} = {}, callback) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'kicad-comment-'));
  fs.writeFileSync(path.join(directory, 'manifest.json'), JSON.stringify(data));
  fs.writeFileSync(path.join(directory, image), imageContent);
  const calls = [];
  const method = (name, result = {}) => async (args) => { calls.push({ name, args }); return { data: result }; };
  let pullReads = 0;
  const github = { rest: {
    pulls: { get: async (args) => {
      calls.push({ name: 'pulls.get', args });
      pullReads += 1;
      return { data: {
        number: 152, state: 'open', base: { sha: data.base, ref: 'master' },
        head: { sha: pullReads === 1 ? currentHead : headAfterUpload, repo: { full_name: 'usbharu/HomeCockpit' } },
        ...prOverrides,
      } };
    } },
    issues: {
      listComments: 'comments', updateComment: method('updateComment'), createComment: method('createComment'),
    },
    actions: { listWorkflowRunArtifacts: 'artifacts' },
    git: {
      getRef: async (args) => {
        calls.push({ name: 'getRef', args });
        if (branchExists) return { data: { object: { sha: 'old-image-commit' } } };
        throw Object.assign(new Error('not found'), { status: 404 });
      },
      createBlob: method('createBlob', { sha: 'blob' }), createTree: method('createTree', { sha: 'tree' }),
      createCommit: method('createCommit', { sha: 'image-commit' }),
      createRef: method('createRef'), updateRef: method('updateRef'),
    },
  }, paginate: async (endpoint, args) => {
    if (endpoint === 'artifacts') assert.equal(args.run_id, 10);
    return endpoint === 'comments' ? comments : artifacts;
  } };
  const context = {
    repo: { owner: 'usbharu', repo: 'HomeCockpit' }, payload: { workflow_run: {
      id: 10, run_number: 2, event: 'pull_request', path: '.github/workflows/kicad-previews.yml',
      head_sha: data.head, conclusion: 'success', head_repository: { full_name: 'usbharu/HomeCockpit' },
      pull_requests: [{ number: 152, head: { sha: data.head }, base: { ref: 'master' } }], ...runOverrides,
    } },
    serverUrl: 'https://github.com', runId: 99, runNumber: 77,
  };
  try {
    if (prepareDirectory) prepareDirectory(directory);
    const action = () => publish({ github, context, core: { info() {} }, directory });
    if (expectedError) await assert.rejects(action, expectedError);
    else await action();
    await callback(calls);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}

test('publishes PNG to a separate branch and leaves violation comment untouched', async () => {
  await withMock(manifest(), { comments: [
    { id: 1, user: { login: 'github-actions[bot]' }, body: '<!-- homecockpit-kicad-ci -->' },
  ] }, (calls) => {
    assert.equal(calls.find((c) => c.name === 'createRef').args.ref, 'refs/heads/ci-previews/pr-152');
    assert.ok(calls.find((c) => c.name === 'createComment'));
    assert.ok(!calls.find((c) => c.name === 'updateComment'));
    assert.match(calls.find((c) => c.name === 'createComment').args.body, /raw\/image-commit\//);
    assert.match(calls.find((c) => c.name === 'createComment').args.body, /actions\/runs\/10\/artifacts\/3/);
    assert.match(calls.find((c) => c.name === 'createComment').args.body, /CI run #2/);
  });
});

test('rerun updates existing preview comment and branch', async () => {
  await withMock(manifest(), { branchExists: true, comments: [
    { id: 2, user: { login: 'github-actions[bot]' }, body: '<!-- homecockpit-kicad-previews -->' },
  ] }, (calls) => {
    assert.equal(calls.find((c) => c.name === 'updateComment').args.comment_id, 2);
    assert.equal(calls.find((c) => c.name === 'updateRef').args.ref, 'heads/ci-previews/pr-152');
  });
});

test('outdated run does not write images or comments', async () => {
  await withMock(manifest(), { currentHead: 'c'.repeat(40) }, (calls) => {
    assert.deepEqual(calls.map((c) => c.name), ['pulls.get']);
  });
});

test('PR updated during image upload does not receive a stale comment', async () => {
  await withMock(manifest(), { headAfterUpload: 'c'.repeat(40) }, (calls) => {
    assert.ok(calls.find((c) => c.name === 'createBlob'));
    assert.ok(!calls.find((c) => c.name === 'createComment' || c.name === 'updateComment'));
  });
});

test('reverted KiCad changes clear existing preview without creating unrelated comments', async () => {
  const data = manifest();
  data.entries = [];
  await withMock(data, {}, (calls) => assert.deepEqual(calls.map((c) => c.name), ['pulls.get']));
  await withMock(data, { comments: [
    { id: 2, user: { login: 'github-actions[bot]' }, body: '<!-- homecockpit-kicad-previews -->' },
  ] }, (calls) => {
    assert.match(calls.find((c) => c.name === 'updateComment').args.body, /現在、プレビュー対象のKiCad変更はありません/);
    assert.ok(!calls.find((c) => c.name === 'createCommit'));
  });
});

test('publisher rejects forks, unrelated workflows, cancelled runs and missing PR metadata', async () => {
  for (const runOverrides of [
    { head_repository: { full_name: 'someone/fork' } },
    { event: 'push' }, { path: '.github/workflows/unrelated.yml' }, { conclusion: 'cancelled' },
    { pull_requests: [] },
  ]) {
    await withMock(manifest(), { runOverrides }, (calls) => assert.deepEqual(calls, []));
  }
});

test('publisher skips closed or forked PRs and unavailable artifacts without writes', async () => {
  for (const options of [
    { prOverrides: { state: 'closed' } },
    { prOverrides: { head: { sha: manifest().head, repo: { full_name: 'someone/fork' } } } },
    { prOverrides: { base: { ref: 'other-branch' } } },
    { artifacts: [] }, { artifacts: [{ name: 'kicad-previews', id: 3, expired: true }] },
  ]) {
    await withMock(manifest(), options, (calls) => assert.deepEqual(calls.map((c) => c.name), ['pulls.get']));
  }
});

test('failed renderer may still publish its successful images', async () => {
  await withMock(manifest(), { runOverrides: { conclusion: 'failure' } }, (calls) => {
    assert.ok(calls.find((c) => c.name === 'createComment'));
  });
});

test('artifact cannot select a different PR or forge its source revision', async () => {
  const data = manifest();
  data.pr_number = 999;
  await withMock(data, {}, (calls) => assert.equal(calls.find((c) => c.name === 'createComment').args.issue_number, 152));
  await withMock(data, {
    prepareDirectory(directory) {
      fs.writeFileSync(path.join(directory, 'manifest.json'), JSON.stringify({ ...data, head: 'c'.repeat(40) }));
    }, expectedError: /mismatched source revision/,
  }, (calls) => assert.deepEqual(calls.map((c) => c.name), ['pulls.get']));
});

test('publisher rejects path traversal, non-PNG images and symlinks before any write', async () => {
  const data = manifest();
  data.entries[0].images.after['sheet: main'] = '../secret.png';
  await withMock(data, { expectedError: /Invalid preview image name/ }, (calls) => {
    assert.deepEqual(calls.map((c) => c.name), ['pulls.get']);
  });
  await withMock(manifest(), { imageContent: 'not a PNG', expectedError: /Invalid PNG/ }, (calls) => {
    assert.deepEqual(calls.map((c) => c.name), ['pulls.get']);
  });
  await withMock(manifest(), {
    prepareDirectory(directory) {
      fs.unlinkSync(path.join(directory, image));
      fs.symlinkSync(path.join(directory, 'manifest.json'), path.join(directory, image));
    }, expectedError: /Invalid preview file/,
  }, (calls) => assert.deepEqual(calls.map((c) => c.name), ['pulls.get']));
});

test('privileged workflow executes only default-branch code, separate from artifact data', () => {
  const workflows = path.resolve(__dirname, '../../workflows');
  const render = fs.readFileSync(path.join(workflows, 'kicad-previews.yml'), 'utf8');
  const comment = fs.readFileSync(path.join(workflows, 'kicad-preview-comment.yml'), 'utf8');
  assert.doesNotMatch(render, /:\s*write\b/);
  assert.doesNotMatch(render, /\n  comment:/);
  assert.match(comment, /workflow_run:\s*\n\s*workflows: \[KiCad previews\]/);
  // PR conversation comments require PR write access even though the REST URL uses /issues/.
  assert.match(comment, /    permissions:\n(?:      .*\n)*      pull-requests: write\n/);
  assert.match(comment, /ref: \$\{\{ github\.sha \}\}\s*\n\s*path: trusted/);
  assert.match(comment, /require\('\.\/trusted\/\.github\/scripts\/comment-kicad-previews\.cjs'\)/);
  assert.match(comment, /path: \$\{\{ runner\.temp \}\}\/kicad-previews/);
  assert.match(comment, /run-id: \$\{\{ github\.event\.workflow_run\.id \}\}/);
});
