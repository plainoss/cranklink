const assert = require('node:assert/strict');
const { test } = require('node:test');
const prepare = require('./prepare-rolling-release.cjs');

function fixture({ main = 'built-sha', tagError, updateError } = {}) {
  const writes = [];
  const outputs = {};
  return {
    writes,
    outputs,
    context: { repo: { owner: 'example', repo: 'project' }, sha: 'built-sha' },
    core: { info() {}, setOutput: (key, value) => { outputs[key] = value; } },
    github: { rest: { git: {
      async getRef({ ref }) {
        if (ref === 'heads/main') return { data: { object: { sha: main } } };
        assert.equal(ref, 'tags/rolling');
        if (tagError) throw tagError;
        return { data: { object: { sha: 'previous-sha' } } };
      },
      async createRef(params) { writes.push({ create: params }); },
      async updateRef(params) {
        if (updateError) throw updateError;
        writes.push({ update: params });
      },
    } } },
  };
}

test('first release creates the rolling tag at the exact built commit', async () => {
  const f = fixture({ tagError: { status: 404 } });
  await prepare(f);
  assert.deepEqual(f.writes, [{ create: {
    owner: 'example', repo: 'project', ref: 'refs/tags/rolling', sha: 'built-sha',
  } }]);
  assert.equal(f.outputs.publish, 'true');
});

test('subsequent release moves the existing rolling tag', async () => {
  const f = fixture();
  await prepare(f);
  assert.deepEqual(f.writes, [{ update: {
    owner: 'example', repo: 'project', ref: 'tags/rolling', sha: 'built-sha', force: true,
  } }]);
  assert.equal(f.outputs.publish, 'true');
});

test('rerunning an older commit cannot roll back the release', async () => {
  const f = fixture({ main: 'newer-sha' });
  await prepare(f);
  assert.deepEqual(f.writes, []);
  assert.equal(f.outputs.publish, 'false');
});

test('permission failures abort publishing without attempting tag creation', async () => {
  const error = Object.assign(new Error('Forbidden'), { status: 403 });
  const f = fixture({ tagError: error });
  await assert.rejects(prepare(f), error);
  assert.deepEqual(f.writes, []);
  assert.equal(f.outputs.publish, undefined);
});

test('failure to move the tag aborts publishing', async () => {
  const error = new Error('Tag is protected');
  const f = fixture({ updateError: error });
  await assert.rejects(prepare(f), error);
  assert.equal(f.outputs.publish, undefined);
});
