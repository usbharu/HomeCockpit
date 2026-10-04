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

async function withMock(data, { comments = [], currentHead = data.head, branchExists = false } = {}, callback) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'kicad-comment-'));
  fs.writeFileSync(path.join(directory, 'manifest.json'), JSON.stringify(data));
  fs.writeFileSync(path.join(directory, image), 'PNG fixture');
  const calls = [];
  const method = (name, result = {}) => async (args) => { calls.push({ name, args }); return { data: result }; };
  const github = { rest: {
    pulls: { get: method('pulls.get', { head: { sha: currentHead } }) },
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
  }, paginate: async (endpoint) => endpoint === 'comments' ? comments : [{ name: 'kicad-previews', id: 3 }] };
  const context = {
    repo: { owner: 'usbharu', repo: 'HomeCockpit' }, payload: { pull_request: { number: 152, base: { sha: data.base } } },
    serverUrl: 'https://github.com', runId: 10, runNumber: 2,
  };
  try {
    await publish({ github, context, core: { info() {} }, directory });
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
