const chalk = require('chalk');
const { Option } = require('commander');
const fs = require('fs');
const path = require('path');
const {
  createIssuesTable,
  displayIssueDetails,
  formatIssueAsMarkdown,
  buildJQL
} = require('../../../lib/utils');
const { readDescriptionFile } = require('./shared');
const { readFieldsFile, validateCreateFields } = require('./create-fields');

async function listIssues(client, io, options) {
  const spinner = io.spinner('Fetching issues...');

  try {
    const jql = buildJQL(options);
    const limit = parseInt(options.limit) || 20;
    const result = await client.searchIssues(jql, {
      maxResults: limit
    });

    spinner.stop();

    if (result.issues.length === 0) {
      if (options.format === 'json') {
        io.out(JSON.stringify({ total: result.total || 0, issues: [] }, null, 2));
      } else {
        io.info('No issues found');
      }
      return;
    }

    if (options.format === 'json') {
      io.out(JSON.stringify({
        total: result.total,
        issues: result.issues.map(issue => ({
          key: issue.key,
          summary: issue.fields.summary,
          status: issue.fields.status?.name,
          assignee: issue.fields.assignee?.displayName || null,
          priority: issue.fields.priority?.name || null,
          created: issue.fields.created,
          updated: issue.fields.updated
        }))
      }, null, 2));
    } else {
      io.out(chalk.bold(`\nFound ${result.issues.length} issues (showing ${Math.min(result.issues.length, limit)}):\n`));

      const table = createIssuesTable(result.issues);
      io.out(table.toString());
    }

    if (options.format !== 'json' && result.total > result.issues.length) {
      io.info(`Showing ${result.issues.length} of ${result.total} total issues`);
    }

  } catch (err) {
    spinner.stop();
    throw err;
  }
}

async function getIssue(client, io, issueKey, options = {}) {
  const spinner = options.format === 'json' ? null : io.spinner(`Fetching issue ${issueKey}...`);

  try {
    const issue = await client.getIssue(issueKey, { allFields: options.format === 'json' });
    spinner?.stop();

    if (options.format === 'json') {
      const content = JSON.stringify(issue);
      if (options.output) fs.writeFileSync(path.resolve(options.output), content + '\n', 'utf8');
      else io.out(content);
    } else if (options.output) {
      const outputPath = path.resolve(options.output);
      const content = formatIssueAsMarkdown(issue);

      fs.writeFileSync(outputPath, content, 'utf8');
      io.success(`Issue ${issueKey} saved to ${outputPath}`);
    } else if (options.format === 'markdown') {
      const markdown = formatIssueAsMarkdown(issue);
      io.out('\n' + markdown);
    } else {
      displayIssueDetails(issue, io);
    }

  } catch (err) {
    spinner?.stop();
    throw err;
  }
}

async function createIssue(client, io, factory, options = {}) {
  const config = factory.getConfig();

  if (!options.project || !options.type || !options.summary) {
    throw new Error(
      'Missing required options for creating an issue.\n\n' +
      'Usage: jira issue create --project <KEY> --type <TYPE> --summary <TEXT>\n\n' +
      'Required options:\n' +
      '  --project <KEY>         Project key (e.g., TEST, PROJ)\n' +
      '  --type <TYPE>           Issue type (e.g., Bug, Story, Task)\n' +
      '  --summary <TEXT>        Issue summary\n\n' +
      'Optional:\n' +
      '  --description <TEXT>    Issue description\n' +
      '  --description-file <PATH>  Read description from file\n' +
      '  --assignee <USER>       Assignee username\n' +
      '  --priority <PRIORITY>   Priority level\n\n' +
      'Example:\n' +
      '  jira issue create --project TEST --type Bug --summary "Login fails"'
    );
  }

  let description;
  if (options.description && options.descriptionFile) {
    throw new Error('Cannot use both --description and --description-file. Please use only one.');
  }

  if (options.descriptionFile) {
    description = readDescriptionFile(options.descriptionFile);
  } else if (options.description) {
    description = options.description;
  }

  const issueData = {
    fields: {
      project: { key: options.project },
      issuetype: { name: options.type },
      summary: options.summary
    }
  };
  if (description !== undefined) issueData.fields.description = description;

  if (options.assignee) {
    issueData.fields.assignee = { name: options.assignee };
  }

  if (options.priority) {
    issueData.fields.priority = { name: options.priority };
  }

  const fields = options.fieldsFile ? readFieldsFile(options.fieldsFile) : {};
  if (options.fieldsFile) {
    // Check collisions before loading metadata; the file cannot override CLI flags.
    for (const id of Object.keys(fields)) {
      if (Object.prototype.hasOwnProperty.call(issueData.fields, id)) {
        throw new Error(`Field "${id}" is already set by a command option; remove the duplicate from --fields-file.`);
      }
    }
  }
  const metadata = await client.getCreateFields(options.project, options.type);
  const richTextFields = validateCreateFields(fields, metadata, issueData.fields);
  if (richTextFields.length) issueData.richTextFields = richTextFields;
  Object.assign(issueData.fields, fields);
  issueData.fields.issuetype = { id: metadata.issueType.id };

  if (options.dryRun) {
    if (options.format === 'json') {
      io.out(JSON.stringify({ dryRun: true, project: metadata.project, issueType: metadata.issueType,
        fields: issueData.fields, richTextFields: richTextFields }));
    } else {
      io.out('Validated create payload (no issue created):');
      io.out(JSON.stringify(issueData.fields, null, 2));
    }
    return;
  }

  const spinner = options.format === 'json' ? null : io.spinner('Creating issue...');
  let result;
  try {
    result = await client.createIssue(issueData);
  } finally {
    spinner?.stop();
  }

  if (options.format === 'json') {
    io.out(JSON.stringify({ key: result.key, url: `${config.get('server')}/browse/${result.key}` }));
    return;
  }
  io.success(`Issue created: ${result.key}`);
  io.out(`URL: ${config.get('server')}/browse/${result.key}`);
}

async function listCreateFields(client, io, options) {
  if (!options.project || !options.type) {
    throw new Error('Specify --project <KEY> and --type <TYPE> to discover create fields.');
  }
  const metadata = await client.getCreateFields(options.project, options.type);
  if (options.format === 'json') {
    io.out(JSON.stringify(metadata));
  } else {
    io.out(`Create fields for ${metadata.project} / ${metadata.issueType.name} (${metadata.issueType.id}):`);
    metadata.fields.forEach(field => io.out(`${field.id} | ${field.name} | ${field.required ? 'required' : 'optional'} | ${field.schema?.custom || field.schema?.type || 'unknown'}`));
  }
}

async function updateIssue(client, io, issueKey, options = {}) {
  if (!options.summary && !options.description && !options.descriptionFile &&
      !options.assignee && !options.priority) {
    throw new Error(
      'At least one field must be specified for update.\n\n' +
      'Usage: jira issue edit <KEY> [options]\n\n' +
      'Available options:\n' +
      '  --summary <TEXT>          New summary\n' +
      '  --description <TEXT>      New description\n' +
      '  --description-file <PATH> Read description from file\n' +
      '  --assignee <USER>         New assignee\n' +
      '  --priority <PRIORITY>     New priority\n\n' +
      'Example:\n' +
      '  jira issue edit PROJ-123 --summary "Updated summary"\n' +
      '  jira issue edit PROJ-123 --description-file ./updated-spec.md'
    );
  }

  const spinner = io.spinner(`Loading issue ${issueKey}...`);
  const issue = await client.getIssue(issueKey);
  spinner.stop();

  io.out(chalk.bold(`\nUpdating issue: ${issue.key}`));
  io.out(`Current summary: ${issue.fields.summary}\n`);

  const updateData = { fields: {} };
  let hasChanges = false;

  if (options.summary && options.summary !== issue.fields.summary) {
    updateData.fields.summary = options.summary;
    hasChanges = true;
  }

  if (options.description && options.descriptionFile) {
    throw new Error('Cannot use both --description and --description-file. Please use only one.');
  }

  if (options.descriptionFile) {
    const description = readDescriptionFile(options.descriptionFile);
    if (description !== (issue.fields.description || '')) {
      updateData.fields.description = description;
      hasChanges = true;
    }
  } else if (options.description && options.description !== (issue.fields.description || '')) {
    updateData.fields.description = options.description;
    hasChanges = true;
  }

  if (options.assignee) {
    updateData.fields.assignee = { name: options.assignee };
    hasChanges = true;
  }

  if (options.priority) {
    updateData.fields.priority = { name: options.priority };
    hasChanges = true;
  }

  if (!hasChanges) {
    io.info('No changes made');
    return;
  }

  const updateSpinner = io.spinner('Updating issue...');
  await client.updateIssue(issueKey, updateData);
  updateSpinner.stop();

  io.success(`Issue ${issueKey} updated successfully`);
}

async function deleteIssue(client, io, issueKey, options = {}) {
  const spinner = io.spinner(`Loading issue ${issueKey}...`);
  const issue = await client.getIssue(issueKey);
  spinner.stop();

  io.out(chalk.bold.red('\nWARNING: You are about to delete this issue:'));
  io.out(`  Key: ${chalk.cyan(issue.key)}`);
  io.out(`  Summary: ${issue.fields.summary}`);
  io.out(`  Type: ${issue.fields.issuetype.name}\n`);

  if (!options.force) {
    throw new Error(
      'Deletion requires --force flag to confirm.\n' +
      `Use: jira issue delete ${issueKey} --force`
    );
  }

  const deleteSpinner = io.spinner('Deleting issue...');
  await client.deleteIssue(issueKey);
  deleteSpinner.stop();

  io.success(`Issue ${issueKey} deleted successfully`);
}

function register(command, factory) {
  command
    .command('list')
    .description('list issues with advanced filtering\n\n' +
      'Examples:\n' +
      '  $ jira issue list                              # List recent issues\n' +
      '  $ jira issue list --assignee=currentUser      # Your assigned issues\n' +
      '  $ jira issue list --status=Open --limit=50    # Open issues (max 50)\n' +
      '  $ jira issue list --project=TEST --type=Bug   # Bugs in TEST project')
    .alias('ls')
    .option('--project <project>', 'filter by project key')
    .option('--assignee <assignee>', 'filter by assignee (use "currentUser" for yourself)')
    .option('--status <status>', 'filter by status (e.g., Open, In Progress)')
    .option('--type <type>', 'filter by issue type (e.g., Bug, Story)')
    .option('--reporter <reporter>', 'filter by reporter (use "currentUser" for yourself)')
    .option('--priority <priority>', 'filter by priority (e.g., High, Medium)')
    .option('--created <date>', 'created since date (e.g., -7d, 2023-01-01)')
    .option('--updated <date>', 'updated since date (e.g., -7d, 2023-01-01)')
    .option('--limit <limit>', 'limit number of results (default: 20)', '20')
    .addOption(new Option('--format <format>', 'output format (table, json)')
      .choices(['table', 'json'])
      .default('table'))
    .option('--jql <query>', 'custom JQL expression, AND-composed with other filters')
    .action(async (options) => {
      const io = factory.getIOStreams();
      const client = await factory.getJiraClient();

      try {
        await listIssues(client, io, options);
      } catch (err) {
        io.error(`Failed to list issues: ${err.message}`);
        process.exit(1);
      }
    });

  command
    .command('view <key>')
    .description('view issue details\n\n' +
      'Examples:\n' +
      '  $ jira issue view PROJ-123                           # View in terminal\n' +
      '  $ jira issue view PROJ-123 --format markdown         # View as markdown\n' +
      '  $ jira issue view PROJ-123 --output ./issue.md       # Save to file\n' +
      '  $ jira issue view PROJ-123 --format markdown --output ./issue.md')
    .alias('show')
    .addOption(new Option('--format <format>', 'output format (terminal, markdown, json)')
      .choices(['terminal', 'markdown', 'json']).default('terminal'))
    .option('--output <path>', 'save to file instead of displaying')
    .action(async (key, options) => {
      const io = factory.getIOStreams();
      const client = await factory.getJiraClient();

      try {
        await getIssue(client, io, key, options);
      } catch (err) {
        io.error(options.format === 'json' ? JSON.stringify({ error: 'get_issue_failed', message: err.message }) : `Failed to get issue: ${err.message}`);
        process.exit(1);
      }
    });

  command
    .command('create-fields')
    .description('discover create-screen fields, IDs, schemas and allowed values for a project and issue type')
    .requiredOption('--project <key>', 'project key')
    .requiredOption('--type <type>', 'issue type name or ID')
    .addOption(new Option('--format <format>', 'output format (table, json)').choices(['table', 'json']).default('table'))
    .action(async (options) => {
      const io = factory.getIOStreams();
      try {
        await listCreateFields(await factory.getJiraClient(), io, options);
      } catch (err) {
        io.error(options.format === 'json' ? JSON.stringify({ error: 'create_fields_failed', message: err.message }) : `Failed to discover create fields: ${err.message}`);
        process.exit(1);
      }
    });

  command
    .command('create')
    .description('create a new issue\n\n' +
      'Examples:\n' +
      '  $ jira issue create --project=TEST --type=Bug --summary="Login fails"\n' +
      '  $ jira issue create --project=TEST --type=Bug \\\n' +
      '                      --summary="Login fails on Safari"\n' +
      '  $ jira issue create --project=PROJ --type=Story \\\n' +
      '                      --summary="Add user profile page" \\\n' +
      '                      --description="Users need a profile page to manage settings" \\\n' +
      '                      --assignee=john.doe --priority=High\n' +
      '  $ jira issue create --project=PROJ --type=Story \\\n' +
      '                      --summary="Add feature" \\\n' +
      '                      --description-file=./feature-spec.md')
    .alias('new')
    .option('--project <project>', 'project key (e.g., TEST, PROJ)')
    .option('--type <type>', 'issue type (e.g., Bug, Story, Task)')
    .option('--summary <summary>', 'issue summary (required)')
    .option('--description <description>', 'issue description (optional)')
    .option('--description-file <path>', 'read description from file (optional)')
    .option('--assignee <assignee>', 'assignee username')
    .option('--priority <priority>', 'priority (e.g., High, Medium, Low)')
    .option('--fields-file <path>', 'JSON object of additional Jira create-field IDs and values (validated against create metadata)')
    .option('--dry-run', 'validate and display the issue fields without creating an issue')
    .addOption(new Option('--format <format>', 'output format (text, json)').choices(['text', 'json']).default('text'))
    .action(async (options) => {
      const io = factory.getIOStreams();
      const client = await factory.getJiraClient();

      try {
        await createIssue(client, io, factory, options);
      } catch (err) {
        io.error(options.format === 'json' ? JSON.stringify({ error: 'create_issue_failed', message: err.message }) : `Failed to create issue: ${err.message}`);
        process.exit(1);
      }
    });

  command
    .command('edit <key>')
    .description('edit issue')
    .alias('update')
    .option('--summary <summary>', 'new summary')
    .option('--description <description>', 'new description')
    .option('--description-file <path>', 'read description from file')
    .option('--assignee <assignee>', 'new assignee')
    .option('--priority <priority>', 'new priority')
    .action(async (key, options) => {
      const io = factory.getIOStreams();
      const client = await factory.getJiraClient();

      try {
        await updateIssue(client, io, key, options);
      } catch (err) {
        io.error(`Failed to update issue: ${err.message}`);
        process.exit(1);
      }
    });

  command
    .command('delete <key>')
    .description('delete issue')
    .alias('rm')
    .option('-f, --force', 'force delete without confirmation')
    .action(async (key, options) => {
      const io = factory.getIOStreams();
      const client = await factory.getJiraClient();

      try {
        await deleteIssue(client, io, key, options);
      } catch (err) {
        io.error(`Failed to delete issue: ${err.message}`);
        process.exit(1);
      }
    });
}

module.exports = {
  register,
  listIssues,
  getIssue,
  createIssue,
  listCreateFields,
  updateIssue,
  deleteIssue
};
