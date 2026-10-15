function normalizeName(value) {
  return value.trim().toLowerCase();
}

function formatAvailableTransitions(transitions) {
  if (transitions.length === 0) return 'none';

  return transitions
    .map(transition => `${transition.name} -> ${transition.to?.name || 'unknown status'}`)
    .join(', ');
}

function findTransition(transitions, value) {
  const idMatches = transitions.filter(transition => String(transition.id) === value);
  if (idMatches.length === 1) return idMatches[0];

  const normalizedValue = normalizeName(value);
  const statusMatches = transitions.filter(transition =>
    transition.to?.name && normalizeName(transition.to.name) === normalizedValue
  );
  const nameMatches = transitions.filter(transition =>
    transition.name && normalizeName(transition.name) === normalizedValue
  );
  const matches = Array.from(new Set([...statusMatches, ...nameMatches]));

  if (matches.length === 0) {
    throw new Error(
      `No available transition matches "${value}". ` +
      `Available transitions: ${formatAvailableTransitions(transitions)}`
    );
  }

  if (matches.length > 1) {
    const choices = matches.map(transition =>
      `${transition.name} (ID: ${transition.id}, status: ${transition.to?.name || 'unknown'})`
    ).join(', ');
    throw new Error(`Transition "${value}" is ambiguous: ${choices}. Use an exact transition ID.`);
  }

  return matches[0];
}

async function transitionIssue(client, io, issueKey, value) {
  const spinner = io.spinner(`Finding transitions for ${issueKey}...`);

  try {
    const transitions = await client.getTransitions(issueKey);
    const transition = findTransition(transitions, value);

    spinner.stop();
    spinner.start(`Moving ${issueKey} to ${transition.to.name}...`);
    await client.transitionIssue(issueKey, transition.id);
    spinner.stop();

    io.success(`${issueKey} moved to ${transition.to.name}`);
    return transition;
  } catch (error) {
    spinner.stop();
    throw error;
  }
}

async function listIssueTransitions(client, io, issueKey, options = {}) {
  const spinner = io.spinner(`Finding transitions for ${issueKey}...`);

  try {
    const transitions = await client.getTransitions(issueKey);
    spinner.stop();

    if (options.format === 'json') {
      io.out(JSON.stringify({ issueKey, transitions }, null, 2));
      return transitions;
    }

    if (transitions.length === 0) {
      io.info(`No available transitions for ${issueKey}`);
      return transitions;
    }

    io.out(`Available transitions for ${issueKey}:`);
    transitions.forEach(transition => {
      io.out(`ID: ${transition.id} | Name: ${transition.name || 'unknown'} | ` +
        `To: ${transition.to?.name || 'unknown status'}`);
    });
    return transitions;
  } catch (error) {
    spinner.stop();
    throw error;
  }
}

function register(parent, factory) {
  parent
    .command('transitions <key>')
    .description('list workflow transitions currently available for an issue\n\n' +
      'Examples:\n' +
      '  $ jira issue transitions PROJ-123\n' +
      '  $ jira issue transitions PROJ-123 --format json')
    .option('--format <format>', 'output format (table, json)', 'table')
    .action(async (key, options) => {
      const io = factory.getIOStreams();
      const client = await factory.getJiraClient();

      try {
        await listIssueTransitions(client, io, key, options);
      } catch (err) {
        io.error(`Failed to list transitions: ${err.message}`);
        process.exit(1);
      }
    });

  parent
    .command('transition <key>')
    .description('transition an issue by ID, destination status, or transition name\n\n' +
      'Examples:\n' +
      '  $ jira issue transition PROJ-123 --to "In Progress"\n' +
      '  $ jira issue transition PROJ-123 --to "Start Progress"\n' +
      '  $ jira issue transition PROJ-123 --to 21')
    .alias('move')
    .requiredOption('--to <transition>', 'transition ID, destination status, or transition name')
    .action(async (key, options) => {
      const io = factory.getIOStreams();
      const client = await factory.getJiraClient();

      try {
        await transitionIssue(client, io, key, options.to);
      } catch (err) {
        io.error(`Failed to transition issue: ${err.message}`);
        process.exit(1);
      }
    });
}

module.exports = {
  register,
  transitionIssue,
  listIssueTransitions,
  findTransition,
  formatAvailableTransitions
};
