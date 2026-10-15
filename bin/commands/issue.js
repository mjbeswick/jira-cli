const { Command } = require('commander');
const crud = require('./issue/crud');
const comment = require('./issue/comment');
const link = require('./issue/link');
const transition = require('./issue/transition');

function createIssueCommand(factory) {
  const command = new Command('issue')
    .description('Manage JIRA issues')
    .alias('i');

  crud.register(command, factory);
  comment.register(command, factory);
  link.register(command, factory);
  transition.register(command, factory);

  return command;
}

module.exports = createIssueCommand;
