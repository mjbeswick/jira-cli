const fs = require('fs');
const path = require('path');

function readFieldsFile(filePath) {
  const absolutePath = path.resolve(filePath);
  let contents;
  try {
    contents = fs.readFileSync(absolutePath, 'utf8');
  } catch (error) {
    throw new Error(`Cannot read fields file ${absolutePath}: ${error.message}`);
  }
  let fields;
  try {
    fields = JSON.parse(contents);
  } catch (error) {
    throw new Error(`Fields file ${absolutePath} is not valid JSON: ${error.message}`);
  }
  if (!fields || typeof fields !== 'object' || Array.isArray(fields) || !Object.keys(fields).length) {
    throw new Error('Fields file must contain a non-empty JSON object keyed by Jira field ID.');
  }
  return fields;
}

function validateCreateFields(fields, metadata, baseFields) {
  const entries = new Map(metadata.fields.map(field => [field.id, field]));
  const richTextFields = [];
  for (const [id, value] of Object.entries(fields)) {
    if (!/^[a-z][a-z0-9_]*$/i.test(id) || ['__proto__', 'constructor', 'prototype'].includes(id)) {
      throw new Error(`Invalid Jira field ID "${id}".`);
    }
    if (Object.prototype.hasOwnProperty.call(baseFields, id)) {
      throw new Error(`Field "${id}" is already set by a command option; remove the duplicate from --fields-file.`);
    }
    const field = entries.get(id);
    if (!field) throw new Error(`Field "${id}" is not on the create screen for ${metadata.project} / ${metadata.issueType.name}.`);
    if (value === null || value === undefined || value === '') throw new Error(`Field "${field.name}" (${id}) must have a value.`);
    const schema = field.schema;
    const customType = schema?.custom?.split(':').pop();
    if (id.startsWith('customfield_') && !schema) {
      throw new Error(`No schema available for "${field.name}" (${id}); cannot safely validate this field.`);
    }
    if (customType === 'textarea') {
      if (typeof value !== 'string' && !(value && typeof value === 'object' && value.type === 'doc')) {
        throw new Error(`Field "${field.name}" (${id}) requires text or an ADF document.`);
      }
      if (typeof value === 'string') richTextFields.push(id);
    } else if (customType === 'textfield' && typeof value !== 'string') {
      throw new Error(`Field "${field.name}" (${id}) requires a string.`);
    } else if (schema?.type === 'number' && (typeof value !== 'number' || !Number.isFinite(value))) {
      throw new Error(`Field "${field.name}" (${id}) requires a number.`);
    } else if (['string', 'date', 'datetime'].includes(schema?.type) && typeof value !== 'string') {
      throw new Error(`Field "${field.name}" (${id}) requires a string.`);
    } else if (schema?.type === 'option') {
      if (!value || typeof value !== 'object' || Array.isArray(value) || !('id' in value || 'value' in value)) {
        throw new Error(`Field "${field.name}" (${id}) requires an option object such as {"id":"..."}.`);
      }
    } else if (schema?.type === 'array' && !Array.isArray(value)) {
      throw new Error(`Field "${field.name}" (${id}) requires an array.`);
    }
    if (Array.isArray(field.allowedValues) && field.allowedValues.length && schema?.type === 'option') {
      if (!field.allowedValues.some(option =>
        (value.id !== undefined && String(option.id) === String(value.id)) ||
        (value.value !== undefined && option.value === value.value))) {
        throw new Error(`Field "${field.name}" (${id}) does not accept the supplied option. Use create-fields to inspect allowedValues.`);
      }
    }
  }
  const combined = { ...baseFields, ...fields };
  const missing = metadata.fields.filter(field => field.required && !field.hasDefaultValue &&
    (combined[field.id] === undefined || combined[field.id] === null || combined[field.id] === '' ||
      (Array.isArray(combined[field.id]) && combined[field.id].length === 0)));
  if (missing.length) {
    throw new Error(`Missing required create fields: ${missing.map(field => `${field.name} (${field.id})`).join(', ')}. Use jira issue create-fields --project ${metadata.project} --type "${metadata.issueType.name}" --format json.`);
  }
  return richTextFields;
}

module.exports = { readFieldsFile, validateCreateFields };
