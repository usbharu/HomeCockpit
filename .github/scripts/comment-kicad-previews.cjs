const fs = require('node:fs');
const path = require('node:path');

const MARKER = '<!-- homecockpit-kicad-previews -->';
const escape = (text) => String(text).replace(/[&<>"|`\r\n]/g, (char) => `&#${char.charCodeAt(0)};`);

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
  const manifest = JSON.parse(fs.readFileSync(path.join(directory, 'manifest.json'), 'utf8'));
  const repo = context.repo;
  const pr = context.payload.pull_request;
  // A rerun of an older commit must not replace the latest preview.
  const current = await github.rest.pulls.get({ ...repo, pull_number: pr.number });
  if (current.data.head.sha !== manifest.head) {
    core.info('Skipping previews for an outdated PR revision');
    return;
  }
  const comments = await github.paginate(github.rest.issues.listComments, {
    ...repo, issue_number: pr.number, per_page: 100,
  });
  const existing = comments.find((comment) =>
    comment.user?.login === 'github-actions[bot]' && comment.body?.includes(MARKER));
  if (!manifest.entries.length && !existing) return;

  const names = imageNames(manifest);
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
    for (const name of names) {
      if (path.basename(name) !== name || !/^[a-f0-9]{20}-(before|after)-[a-f0-9]{12}\.png$/.test(name)) {
        throw new Error(`Invalid preview image name: ${name}`);
      }
      const blob = await github.rest.git.createBlob({
        ...repo, content: fs.readFileSync(path.join(directory, name)).toString('base64'), encoding: 'base64',
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
  const runUrl = `${rootUrl}/actions/runs/${context.runId}`;
  const artifacts = await github.paginate(github.rest.actions.listWorkflowRunArtifacts, {
    ...repo, run_id: context.runId, per_page: 100,
  });
  const artifact = artifacts.find((item) => item.name === 'kicad-previews');
  const body = buildBody(manifest, `${rootUrl}/raw/${imageCommit}`, runUrl,
    artifact ? `${runUrl}/artifacts/${artifact.id}` : runUrl, context.runNumber);
  if (existing) {
    await github.rest.issues.updateComment({ ...repo, comment_id: existing.id, body });
  } else {
    await github.rest.issues.createComment({ ...repo, issue_number: pr.number, body });
  }
};

module.exports.buildBody = buildBody;
module.exports.imageNames = imageNames;
