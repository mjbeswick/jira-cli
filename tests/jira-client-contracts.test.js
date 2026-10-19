const http = require('http');
const JiraClient = require('../lib/jira-client');

describe('Jira REST write contracts', () => {
  let server;
  let config;
  let requests;
  let reply;

  beforeEach(async () => {
    requests = [];
    reply = jest.fn(() => ({ status: 200, body: { key: 'TEST-1' } }));
    server = http.createServer((req, res) => {
      let body = '';
      req.on('data', chunk => { body += chunk; });
      req.on('end', () => {
        const request = { method: req.method, url: req.url, data: body ? JSON.parse(body) : undefined };
        requests.push(request);
        const response = reply(request);
        res.writeHead(response.status, response.headers || { 'Content-Type': 'application/json' });
        res.end(typeof response.body === 'string' ? response.body : JSON.stringify(response.body));
      });
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    config = { server: `http://127.0.0.1:${server.address().port}` };
  });

  afterEach(async () => {
    await new Promise(resolve => server.close(resolve));
  });

  function createClient(apiVersion = 'auto') {
    const client = new JiraClient({ ...config, apiVersion });
    // These fixtures must use the local server even if the host has a proxy.
    client.clientV2.defaults.proxy = false;
    client.clientV3.defaults.proxy = false;
    return client;
  }

  const writes = [
    ['issue edit', 'PUT', '/issue/TEST-1', client => client.updateIssue('TEST-1', { fields: { summary: 'Updated' } })],
    ['issue delete', 'DELETE', '/issue/TEST-1', client => client.deleteIssue('TEST-1')],
    ['issue transition', 'POST', '/issue/TEST-1/transitions', client => client.transitionIssue('TEST-1', '21')],
    ['comment edit', 'PUT', '/issue/TEST-1/comment/10000', client => client.updateComment('TEST-1', '10000', 'Updated')],
    ['comment delete', 'DELETE', '/issue/TEST-1/comment/10000', client => client.deleteComment('TEST-1', '10000')],
    ['remote link edit', 'PUT', '/issue/TEST-1/remotelink/10000', client => client.updateRemoteLink('TEST-1', '10000', { object: { title: 'Updated' } })],
    ['remote link delete', 'DELETE', '/issue/TEST-1/remotelink/10000', client => client.deleteRemoteLink('TEST-1', '10000')]
  ];

  test.each([2, 3])('gets available transitions using Jira API v%i', async version => {
    const client = createClient(version);
    reply.mockReturnValue({
      status: 200,
      body: { transitions: [{ id: '21', name: 'Start Progress', to: { name: 'In Progress' } }] }
    });

    const transitions = await client.getTransitions('TEST-1');

    expect(requests).toEqual([{
      method: 'GET', url: `/rest/api/${version}/issue/TEST-1/transitions`, data: undefined
    }]);
    expect(transitions).toEqual([
      { id: '21', name: 'Start Progress', to: { name: 'In Progress' } }
    ]);
  });

  test.each([2, 3])('transitions an issue using Jira API v%i', async version => {
    const client = createClient(version);
    reply.mockReturnValue({ status: 204, body: '' });

    await client.transitionIssue('TEST-1', 21);

    expect(requests).toEqual([{
      method: 'POST', url: `/rest/api/${version}/issue/TEST-1/transitions`,
      data: { transition: { id: '21' } }
    }]);
  });

  test('does not retry or replay a transition when the first API version rejects it', async () => {
    const client = createClient();
    reply.mockReturnValue({ status: 404, body: { errorMessages: ['Transition unavailable'] } });

    await expect(client.transitionIssue('TEST-1', '21')).rejects.toMatchObject({ status: 404 });

    expect(requests).toHaveLength(1);
    expect(requests[0]).toMatchObject({
      method: 'POST', url: '/rest/api/3/issue/TEST-1/transitions',
      data: { transition: { id: '21' } }
    });
  });

  test('does not report success for a transition that receives an HTML response', async () => {
    const client = createClient();
    reply.mockReturnValue({
      status: 200,
      headers: { 'Content-Type': 'text/html' },
      body: '<html>Login required</html>'
    });

    await expect(client.transitionIssue('TEST-1', '21')).rejects.toThrow(
      'Jira returned a non-JSON response; transition success could not be confirmed.'
    );

    expect(requests).toHaveLength(1);
    expect(requests[0].method).toBe('POST');
  });

  test('does not retry a transition after a retryable server failure', async () => {
    const client = createClient();
    reply.mockReturnValue({ status: 503, body: { errorMessages: ['Unavailable'] } });

    await expect(client.transitionIssue('TEST-1', '21')).rejects.toMatchObject({ status: 503 });

    expect(requests).toHaveLength(1);
    expect(requests[0].method).toBe('POST');
  });

  describe.each([2, 3])('auto mode currently using v%i', version => {
    test.each(writes)('%s sends exactly one request after a real HTTP 204', async (_name, method, path, write) => {
      const client = createClient();
      client.apiVersion = version;
      // Axios decodes HTTP 204 as data: ''. A misleading Content-Type must
      // not turn this completed mutation into a second request to another API.
      reply.mockReturnValue({ status: 204, headers: { 'Content-Type': 'text/html' }, body: '' });

      await write(client);

      expect(requests).toHaveLength(1);
      expect(requests[0]).toMatchObject({ method, url: `/rest/api/${version}${path}` });
      expect(client.apiVersion).toBe(version);
    });
  });

  test.each([200, 201, 202, 205])('empty HTTP %i success does not replay a write', async status => {
    const client = createClient();
    reply.mockReturnValue({ status, body: '' });

    const response = await client.requestApi('post', '/issue', { fields: { summary: 'New' } });

    expect(response.status).toBe(status);
    expect(response.data).toBe('');
    expect(requests).toHaveLength(1);
    expect(client.apiVersion).toBe(3);
  });

  const issueWrites = [
    ['create', 'POST', '/issue', (client, data) => client.createIssue(data)],
    ['update', 'PUT', '/issue/TEST-1', (client, data) => client.updateIssue('TEST-1', data)]
  ];

  test.each([2, 3])('sends textarea custom fields with the correct v%i representation', async version => {
    const client = createClient(version);
    const data = { fields: { project: { key: 'TEST' }, customfield_10010: 'Impact\nDetails', customfield_10011: { id: '2' } }, richTextFields: ['customfield_10010'] };
    await client.createIssue(data);
    expect(requests).toHaveLength(1);
    expect(requests[0].data).toEqual({ fields: {
      project: { key: 'TEST' }, customfield_10010: version === 3 ? client.toADF('Impact\nDetails') : 'Impact\nDetails',
      customfield_10011: { id: '2' }
    } });
    expect(data.fields.customfield_10010).toBe('Impact\nDetails');
  });

  test('does not retry issue creation after an ambiguous server error', async () => {
    const client = createClient(3);
    reply.mockReturnValue({ status: 503, body: { errorMessages: ['Unavailable'] } });
    await expect(client.createIssue({ fields: { summary: 'New' } })).rejects.toMatchObject({ status: 503 });
    expect(requests).toHaveLength(1);
  });

  test('refuses to claim an issue was created without a key', async () => {
    const client = createClient(3);
    reply.mockReturnValue({ status: 200, headers: { 'Content-Type': 'text/html' }, body: '<html>Login required</html>' });
    await expect(client.createIssue({ fields: { summary: 'New' } })).rejects.toThrow('did not confirm');
    expect(requests).toHaveLength(1);
  });

  test('discovers all paginated Cloud create fields and enriches custom schema', async () => {
    const client = createClient(3);
    reply.mockImplementation(req => {
      if (req.url === '/rest/api/3/field') return { status: 200, body: [{ id: 'customfield_10010', schema: { type: 'string', custom: 'example:textarea' } }] };
      if (req.url.includes('/issuetypes/1?')) {
        const second = req.url.includes('startAt=1');
        return { status: 200, body: { fields: second ? [{ fieldId: 'customfield_10010', name: 'Impact Details', required: true }] : [{ fieldId: 'summary', name: 'Summary', required: true }], total: 2 } };
      }
      return { status: 200, body: { issueTypes: [{ id: '1', name: 'Bug' }], total: 1 } };
    });
    const metadata = await client.getCreateFields('TEST', 'Bug');
    expect(metadata.fields).toHaveLength(2);
    expect(metadata.fields[1].schema.custom).toBe('example:textarea');
    expect(requests).toHaveLength(4);
  });

  test('uses v2 create metadata when configured for Data Center', async () => {
    const client = createClient(2);
    reply.mockReturnValue({ status: 200, body: { projects: [{ key: 'TEST', issuetypes: [{ id: '1', name: 'Bug', fields: { customfield_10011: { name: 'Severity', required: true, schema: { type: 'option' } } } }] }] } });
    const metadata = await client.getCreateFields('TEST', 'Bug');
    expect(metadata.fields[0].id).toBe('customfield_10011');
    expect(requests).toHaveLength(1);
    expect(requests[0].url).toContain('/rest/api/2/issue/createmeta?');
  });

  test('falls back to v2 read-only metadata, then creates exactly once using v2', async () => {
    const client = createClient();
    reply.mockImplementation(req => {
      if (req.url.startsWith('/rest/api/3/issue/createmeta/')) return { status: 404, body: { errorMessages: ['Not found'] } };
      if (req.url.startsWith('/rest/api/2/issue/createmeta?')) return { status: 200, body: { projects: [{ key: 'TEST', issuetypes: [{ id: '1', name: 'Bug', fields: { summary: { name: 'Summary', required: true } } }] }] } };
      return { status: 201, body: { key: 'TEST-1' } };
    });
    await client.getCreateFields('TEST', 'Bug');
    await client.createIssue({ fields: { summary: 'Example' } });
    expect(requests.filter(req => req.method === 'POST')).toEqual([{
      method: 'POST', url: '/rest/api/2/issue', data: { fields: { summary: 'Example' } }
    }]);
  });

  test('view all fields requests explicit *all for agent verification', async () => {
    const client = createClient(3);
    await client.getIssue('TEST-1', { allFields: true });
    expect(requests[0].url).toContain('fields=*all');
  });

  const description = 'First line\nSecond line\n\nNext paragraph';
  const adf = {
    type: 'doc', version: 1, content: [
      { type: 'paragraph', content: [
        { type: 'text', text: 'First line' },
        { type: 'hardBreak' },
        { type: 'text', text: 'Second line' }
      ] },
      { type: 'paragraph', content: [{ type: 'text', text: 'Next paragraph' }] }
    ]
  };

  describe.each(issueWrites)('issue %s description', (_name, method, path, write) => {
    test.each([2, 3])('uses the pinned v%i representation without mutating input', async version => {
      const client = createClient(version);
      const fields = Object.freeze({ summary: 'Example', description });
      const data = Object.freeze({ fields });

      await write(client, data);

      expect(requests).toEqual([{
        method, url: `/rest/api/${version}${path}`,
        data: { fields: { summary: 'Example', description: version === 3 ? adf : description } }
      }]);
      expect(data.fields).toBe(fields);
      expect(data.fields.description).toBe(description);
    });

    if (method !== 'POST') test.each([2, 3])('rebuilds the payload when auto falls back from v%i', async version => {
      const client = createClient();
      client.apiVersion = version;
      const data = { fields: { description } };
      const fallback = version === 3 ? 2 : 3;
      reply.mockReturnValueOnce({ status: 404, body: { errorMessages: ['Endpoint not found'] } })
        .mockReturnValueOnce({ status: method === 'PUT' ? 204 : 201, body: method === 'PUT' ? '' : { key: 'TEST-1' } });

      await write(client, data);

      expect(requests).toEqual([version, fallback].map(apiVersion => ({
        method, url: `/rest/api/${apiVersion}${path}`,
        data: { fields: { description: apiVersion === 3 ? adf : description } }
      })));
      expect(client.apiVersion).toBe(fallback);
      expect(data.fields.description).toBe(description);
    });

    if (method !== 'POST') test('retains HTML fallback and rebuilds the v2 description', async () => {
      const client = createClient();
      reply.mockReturnValueOnce({ status: 200, headers: { 'Content-Type': 'text/html' }, body: '<html>Unsupported API</html>' });

      await write(client, { fields: { description } });

      expect(requests).toHaveLength(2);
      expect(requests[0].data.fields.description).toEqual(adf);
      expect(requests[1]).toEqual({ method, url: `/rest/api/2${path}`, data: { fields: { description } } });
      expect(client.apiVersion).toBe(2);
    });

    test.each([null, adf])('passes through null or pre-built ADF', async value => {
      const client = createClient(3);
      await write(client, { fields: { description: value } });
      expect(requests[0].data.fields.description).toEqual(value);
    });

    test('does not add an omitted description', async () => {
      await write(createClient(3), { fields: { summary: 'Example' } });
      expect(requests[0].data).toEqual({ fields: { summary: 'Example' } });
    });

    test('encodes an empty description without an invalid empty text node', async () => {
      await write(createClient(3), { fields: { description: '' } });
      expect(requests[0].data.fields.description).toEqual({
        type: 'doc', version: 1, content: [{ type: 'paragraph' }]
      });
    });

    test('does not fall back on unrelated validation errors', async () => {
      reply.mockReturnValue({ status: 400, body: { errors: { summary: 'Summary is required' } } });
      await expect(write(createClient(3), { fields: { description } })).rejects.toMatchObject({ status: 400 });
      expect(requests).toHaveLength(1);
    });

    test('does not fall back when pinned to v3', async () => {
      reply.mockReturnValue({ status: 404, body: { errorMessages: ['Not found'] } });
      await expect(write(createClient(3), { fields: { description } })).rejects.toMatchObject({ status: 404 });
      expect(requests).toHaveLength(1);
    });
  });

  test.each([2, 3])('comment edit uses an issue-scoped route and the v%i body', async version => {
    const client = createClient(version);
    await client.updateComment('TEST-1', '10000', description);
    expect(requests).toEqual([{
      method: 'PUT', url: `/rest/api/${version}/issue/TEST-1/comment/10000`,
      data: { body: version === 3 ? adf : description }
    }]);
  });

  test('comment edit preserves the scoped route while rebuilding its fallback body', async () => {
    const client = createClient();
    reply.mockReturnValueOnce({ status: 404, body: { errorMessages: ['Not found'] } });
    await client.updateComment('TEST-1', '10000', description);
    expect(requests.map(request => request.url)).toEqual([
      '/rest/api/3/issue/TEST-1/comment/10000', '/rest/api/2/issue/TEST-1/comment/10000'
    ]);
    expect(requests[0].data.body).toEqual(adf);
    expect(requests[1].data.body).toBe(description);
  });

  test('rejects old comment-ID-only calls before making a request', async () => {
    const client = createClient();
    await expect(client.updateComment('10000', 'Updated text')).rejects.toThrow('Issue key');
    await expect(client.deleteComment('10000')).rejects.toThrow('Issue key');
    expect(requests).toHaveLength(0);
  });
});
