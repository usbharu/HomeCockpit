const fs = require('node:fs');
const path = require('node:path');

const MARKER = '<!-- homecockpit-kicad-previews -->';
const escape = (text) => String(text).replace(/[&<>"|`\\[\]()*_!#\r\n]/g, (char) => `&#${char.charCodeAt(0)};`);
const IMAGE_NAME = /^[a-f0-9]{20}-(before|after)-[a-f0-9]{12}\.png$/;
const PNG_SIGNATURE = Buffer.from('89504e470d0a1a0a', 'hex');

async function prepare({ github, context, core }) {
  const run = context.payload.workflow_run;
  const repository = `${context.repo.owner}/${context.repo.repo}`;
  if (!run || run.event !== 'pull_request' || run.path !== '.github/workflows/kicad-previews.yml'
      || run.head_repository?.full_name !== repository || !['success', 'failure'].includes(run.conclusion)) {
    core.info('Skipping an unsupported preview source run');
    return null;
  }
  // Only GitHub's run metadata identifies the PR. Never accept a number from the artifact.
  const candidate = run.pull_requests?.find((pr) => pr.head.sha === run.head_sha && pr.base.ref === 'master');
  if (!candidate) {
    core.info('No matching PR in the source run metadata');
    return null;
  }
  const { data: pr } = await github.rest.pulls.get({ ...context.repo, pull_number: candidate.number });
  if (pr.state !== 'open' || pr.base.ref !== 'master' || pr.head.repo?.full_name !== repository
      || pr.head.sha !== run.head_sha) {
    core.info('Skipping previews for a closed, forked or outdated PR');
    return null;
  }
  const artifacts = await github.paginate(github.rest.actions.listWorkflowRunArtifacts, {
    ...context.repo, run_id: run.id, per_page: 100,
  });
  const artifact = artifacts.find((item) => item.name === 'kicad-previews' && !item.expired);
  if (!artifact) {
    core.info('The source run did not upload preview data');
    return null;
  }
  return { run, pr, artifact };
}

function readData(file, limit) {
  const stat = fs.lstatSync(file);
  if (!stat.isFile() || stat.size > limit) throw new Error(`Invalid preview file: ${path.basename(file)}`);
  return fs.readFileSync(file);
}

function validateManifest(manifest, head) {
  if (!/^[a-f0-9]{40}$/.test(manifest.base) || manifest.head !== head
      || !Array.isArray(manifest.entries) || manifest.entries.length > 1000) {
    throw new Error('Invalid preview manifest or mismatched source revision');
  }
  for (const entry of manifest.entries) {
    if (typeof entry.source !== 'string' || !/\.kicad_(pcb|sch)$/.test(entry.source)
        || entry.source.length > 1024 || !['added', 'deleted', 'modified', 'related'].includes(entry.status)
        || !entry.images || !entry.errors || typeof entry.errors !== 'object') {
      throw new Error('Invalid preview entry');
    }
    for (const side of ['before', 'after']) {
      const views = entry.images[side];
      if (typeof entry[side] !== 'boolean' || !views || typeof views !== 'object' || Array.isArray(views)
          || Object.keys(views).length > 100) throw new Error('Invalid preview views');
      for (const [label, name] of Object.entries(views)) {
        if (label.length > 1024 || typeof name !== 'string' || !IMAGE_NAME.test(name)) {
          throw new Error('Invalid preview image name');
        }
      }
    }
  }
}

function imageNames(manifest) {
  return [...new Set(manifest.entries.flatMap((entry) =>
    Object.values(entry.images).flatMap((views) => Object.values(views))))].sort();
}

function buildBody(manifest, imageBaseUrl, runUrl, artifactUrl, runNumber) {
  const lines = [
    MARKER, '## KiCad 変更プレビュー', '',
    '基板の表裏と回路図を、PRの変更前後で比較できます。ERC／DRCの検証結果は別コメントを確認してください。', '',
    `比較: \`${manifest.base.slice(0, 12)}\` → \`${manifest.head.slice(0, 12)}\``, '',
    `[Actions の実行結果](${runUrl}) ・ [PNG / SVG をダウンロード](${artifactUrl})`, '',
  ];
  if (!manifest.entries.length) {
    lines.push('このPRには現在、プレビュー対象のKiCad変更はありません。');
  }
  for (const entry of manifest.entries) {
    const status = { added: '追加', deleted: '削除', modified: '変更', related: '関連設計' }[entry.status];
    const section = [`### ${escape(entry.source)}（${status}）`, ''];
    const labels = [...new Set([...Object.keys(entry.images.before), ...Object.keys(entry.images.after)])]
      .sort((a, b) => ({ front: 0, back: 1 }[a] ?? 2) - ({ front: 0, back: 1 }[b] ?? 2) || a.localeCompare(b));
    for (const label of labels) {
      section.push(`**${escape({ front: '表面', back: '裏面' }[label] || label.replace(/^sheet: /, '回路図: '))}**`, '',
        '| 変更前 | 変更後 |', '| --- | --- |');
      const cells = ['before', 'after'].map((side) => {
        const name = entry.images[side][label];
        return name ? `![${side}](${imageBaseUrl}/${encodeURIComponent(name)})`
          : !entry[side] ? (side === 'before' ? '新規追加' : '削除済み')
            : entry.errors[side] ? '画像生成に失敗' : '該当ページなし';
      });
      section.push(`| ${cells.join(' | ')} |`, '');
    }
    for (const side of ['before', 'after']) {
      if (entry.errors[side]) {
        section.push(`${side === 'before' ? '変更前' : '変更後'}の画像生成に失敗しました。Actions の実行ログを確認してください。`, '');
      } else if (entry[side] && !Object.keys(entry.images[side]).length) {
        section.push(`${side === 'before' ? '変更前' : '変更後'}の画像が生成されていません。Actions の実行ログを確認してください。`, '');
      }
    }
    if (lines.join('\n').length + section.join('\n').length > 58000) {
      lines.push('残りの画像はダウンロードした artifact で確認してください。');
      break;
    }
    lines.push(...section);
  }
  lines.push('', `_このコメントは CI run #${runNumber} が自動更新しています。_`);
  return lines.join('\n');
}

module.exports = async function publish({ github, context, core, directory }) {
  const source = await prepare({ github, context, core });
  if (!source) return;
  const { run, pr, artifact } = source;
  const manifest = JSON.parse(readData(path.join(directory, 'manifest.json'), 5 * 1024 * 1024).toString('utf8'));
  validateManifest(manifest, run.head_sha);
  const repo = context.repo;
  const comments = await github.paginate(github.rest.issues.listComments, {
    ...repo, issue_number: pr.number, per_page: 100,
  });
  const existing = comments.find((comment) =>
    comment.user?.login === 'github-actions[bot]' && comment.body?.includes(MARKER));
  if (!manifest.entries.length && !existing) return;

  const names = imageNames(manifest);
  if (names.length > 500) throw new Error('Too many preview images');
  // Validate every image before making any write API calls. Artifact files are data only.
  let totalBytes = 0;
  const images = names.map((name) => {
    const content = readData(path.join(directory, name), 20 * 1024 * 1024);
    totalBytes += content.length;
    if (!content.subarray(0, 8).equals(PNG_SIGNATURE) || totalBytes > 50 * 1024 * 1024) {
      throw new Error('Invalid PNG preview or oversized image payload');
    }
    return { name, content };
  });
  let imageCommit;
  if (names.length) {
    // Use a separate branch from the ERC/DRC images, so either job can run first.
    const branch = `ci-previews/pr-${pr.number}`;
    let parent;
    try {
      parent = (await github.rest.git.getRef({ ...repo, ref: `heads/${branch}` })).data.object.sha;
    } catch (error) {
      if (error.status !== 404) throw error;
    }
    const tree = [];
    for (const { name, content } of images) {
      const blob = await github.rest.git.createBlob({
        ...repo, content: content.toString('base64'), encoding: 'base64',
      });
      tree.push({ path: name, mode: '100644', type: 'blob', sha: blob.data.sha });
    }
    const createdTree = await github.rest.git.createTree({ ...repo, tree });
    const commit = await github.rest.git.createCommit({
      ...repo, message: `ci: update KiCad previews for PR #${pr.number}`,
      tree: createdTree.data.sha, parents: [parent || pr.base.sha],
    });
    imageCommit = commit.data.sha;
    if (parent) {
      await github.rest.git.updateRef({ ...repo, ref: `heads/${branch}`, sha: imageCommit });
    } else {
      await github.rest.git.createRef({ ...repo, ref: `refs/heads/${branch}`, sha: imageCommit });
    }
  }
  const rootUrl = `${context.serverUrl}/${repo.owner}/${repo.repo}`;
  const runUrl = `${rootUrl}/actions/runs/${run.id}`;
  const body = buildBody(manifest, `${rootUrl}/raw/${imageCommit}`, runUrl,
    `${runUrl}/artifacts/${artifact.id}`, run.run_number);
  const latest = await github.rest.pulls.get({ ...repo, pull_number: pr.number });
  if (latest.data.state !== 'open' || latest.data.head.sha !== run.head_sha) {
    core.info('Skipping a PR that changed while its images were being uploaded');
    return;
  }
  if (existing) {
    await github.rest.issues.updateComment({ ...repo, comment_id: existing.id, body });
  } else {
    await github.rest.issues.createComment({ ...repo, issue_number: pr.number, body });
  }
};

module.exports.buildBody = buildBody;
module.exports.imageNames = imageNames;
module.exports.prepare = prepare;
module.exports.validateManifest = validateManifest;
