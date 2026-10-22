// Called by actions/github-script only after all release archives are verified.
module.exports = async ({ github, context, core }) => {
  const repo = context.repo;
  const { data: main } = await github.rest.git.getRef({ ...repo, ref: 'heads/main' });
  if (main.object.sha !== context.sha) {
    core.info('main has advanced; skipping this older rolling build.');
    core.setOutput('publish', 'false');
    return;
  }

  // target_commitish on a release does not move an existing tag.
  try {
    await github.rest.git.getRef({ ...repo, ref: 'tags/rolling' });
  } catch (error) {
    if (error.status !== 404) throw error;
    await github.rest.git.createRef({ ...repo, ref: 'refs/tags/rolling', sha: context.sha });
    core.setOutput('publish', 'true');
    return;
  }
  await github.rest.git.updateRef({ ...repo, ref: 'tags/rolling', sha: context.sha, force: true });
  core.setOutput('publish', 'true');
};
