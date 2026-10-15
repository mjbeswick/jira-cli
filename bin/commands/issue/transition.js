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

function register(parent, factory) {
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
  findTransition,
  formatAvailableTransitions
};
