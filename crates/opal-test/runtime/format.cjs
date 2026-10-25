// pretty-format, as Jest 30 has it: the serializer behind snapshots, matcher
// messages and `.each` titles. Ported from jest/packages/pretty-format/src
// (index.ts, collections.ts and the AsymmetricMatcher, ReactElement,
// ReactTestComponent, DOMElement and DOMCollection plugins), keeping its
// output byte for byte, because a snapshot written by Jest has to read back
// the same here. Colors are left out: nothing opal prints highlights values.
//
// Runs inside each test file's context, like everything the framework loads,
// so `toString` and the built-in prototypes are the test's own.
'use strict';

const toString = Object.prototype.toString;
const toISOString = Date.prototype.toISOString;
const errorToString = Error.prototype.toString;
const regExpToString = RegExp.prototype.toString;

const getConstructorName = val =>
  (typeof val.constructor === 'function' && val.constructor.name) || 'Object';

const isWindow = val => typeof window !== 'undefined' && val === window;

const SYMBOL_REGEXP = /^Symbol\((.*)\)(.*)$/;
const NEWLINE_REGEXP = /\n/gi;

class PrettyFormatPluginError extends Error {
  constructor(message, stack) {
    super(message);
    this.stack = stack;
    this.name = this.constructor.name;
  }
}

const ARRAY_TYPES = new Set([
  '[object Array]',
  '[object ArrayBuffer]',
  '[object DataView]',
  '[object Float32Array]',
  '[object Float64Array]',
  '[object Int8Array]',
  '[object Int16Array]',
  '[object Int32Array]',
  '[object Uint8Array]',
  '[object Uint8ClampedArray]',
  '[object Uint16Array]',
  '[object Uint32Array]',
]);

const printNumber = val => (Object.is(val, -0) ? '-0' : String(val));
const printBigInt = val => String(`${val}n`);
const printFunction = (val, printFunctionName) =>
  printFunctionName ? `[Function ${val.name || 'anonymous'}]` : '[Function]';
const printSymbol = val => String(val).replace(SYMBOL_REGEXP, 'Symbol($1)');
const printError = val => `[${errorToString.call(val)}]`;

function printBasicValue(val, printFunctionName, escapeRegex, escapeString) {
  if (val === true || val === false) return `${val}`;
  if (val === undefined) return 'undefined';
  if (val === null) return 'null';
  const typeOf = typeof val;
  if (typeOf === 'number') return printNumber(val);
  if (typeOf === 'bigint') return printBigInt(val);
  if (typeOf === 'string') {
    if (escapeString) return `"${val.replaceAll(/"|\\/g, '\\$&')}"`;
    return `"${val}"`;
  }
  if (typeOf === 'function') return printFunction(val, printFunctionName);
  if (typeOf === 'symbol') return printSymbol(val);
  const toStringed = toString.call(val);
  if (toStringed === '[object Promise]') return 'Promise {}';
  if (toStringed === '[object WeakMap]') return 'WeakMap {}';
  if (toStringed === '[object WeakSet]') return 'WeakSet {}';
  if (toStringed === '[object Function]' || toStringed === '[object GeneratorFunction]') {
    return printFunction(val, printFunctionName);
  }
  if (toStringed === '[object Symbol]') return printSymbol(val);
  if (toStringed === '[object Date]') {
    return Number.isNaN(+val) ? 'Date { NaN }' : toISOString.call(val);
  }
  if (toStringed === '[object Error]') return printError(val);
  if (toStringed === '[object RegExp]') {
    if (escapeRegex) {
      return regExpToString.call(val).replaceAll(/[$()*+.?[\\\]^{|}]/g, '\\$&');
    }
    return regExpToString.call(val);
  }
  if (val instanceof Error) return printError(val);
  return null;
}

function getKeysOfEnumerableProperties(object, compareKeys) {
  const rawKeys = Object.keys(object);
  const keys = compareKeys === null ? rawKeys : rawKeys.sort(compareKeys);
  for (const symbol of Object.getOwnPropertySymbols(object)) {
    if (Object.getOwnPropertyDescriptor(object, symbol).enumerable) {
      keys.push(symbol);
    }
  }
  return keys;
}

function printIteratorEntries(iterator, config, indentation, depth, refs, printer, separator = ': ') {
  let result = '';
  let width = 0;
  let current = iterator.next();
  if (!current.done) {
    result += config.spacingOuter;
    const indentationNext = indentation + config.indent;
    while (!current.done) {
      result += indentationNext;
      if (width++ === config.maxWidth) {
        result += '…';
        break;
      }
      const name = printer(current.value[0], config, indentationNext, depth, refs);
      const value = printer(current.value[1], config, indentationNext, depth, refs);
      result += name + separator + value;
      current = iterator.next();
      if (!current.done) result += `,${config.spacingInner}`;
      else if (!config.min) result += ',';
    }
    result += config.spacingOuter + indentation;
  }
  return result;
}

function printIteratorValues(iterator, config, indentation, depth, refs, printer) {
  let result = '';
  let width = 0;
  let current = iterator.next();
  if (!current.done) {
    result += config.spacingOuter;
    const indentationNext = indentation + config.indent;
    while (!current.done) {
      result += indentationNext;
      if (width++ === config.maxWidth) {
        result += '…';
        break;
      }
      result += printer(current.value, config, indentationNext, depth, refs);
      current = iterator.next();
      if (!current.done) result += `,${config.spacingInner}`;
      else if (!config.min) result += ',';
    }
    result += config.spacingOuter + indentation;
  }
  return result;
}

function printListItems(list, config, indentation, depth, refs, printer) {
  let result = '';
  list = list instanceof ArrayBuffer || toString.call(list) === '[object ArrayBuffer]'
    ? new DataView(list)
    : list;
  const isDataView = l => toString.call(l) === '[object DataView]';
  const length = isDataView(list) ? list.byteLength : list.length;
  if (length > 0) {
    result += config.spacingOuter;
    const indentationNext = indentation + config.indent;
    for (let i = 0; i < length; i++) {
      result += indentationNext;
      if (i === config.maxWidth) {
        result += '…';
        break;
      }
      if (isDataView(list) || i in list) {
        result += printer(isDataView(list) ? list.getInt8(i) : list[i], config, indentationNext, depth, refs);
      }
      if (i < length - 1) result += `,${config.spacingInner}`;
      else if (!config.min) result += ',';
    }
    result += config.spacingOuter + indentation;
  }
  return result;
}

function printObjectProperties(val, config, indentation, depth, refs, printer) {
  let result = '';
  const keys = getKeysOfEnumerableProperties(val, config.compareKeys);
  if (keys.length > 0) {
    result += config.spacingOuter;
    const indentationNext = indentation + config.indent;
    for (let i = 0; i < keys.length; i++) {
      const key = keys[i];
      const name = printer(key, config, indentationNext, depth, refs);
      const value = printer(val[key], config, indentationNext, depth, refs);
      result += `${indentationNext + name}: ${value}`;
      if (i < keys.length - 1) result += `,${config.spacingInner}`;
      else if (!config.min) result += ',';
    }
    result += config.spacingOuter + indentation;
  }
  return result;
}

function printComplexValue(val, config, indentation, depth, refs, hasCalledToJSON) {
  if (refs.includes(val)) return '[Circular]';
  refs = [...refs, val];
  const hitMaxDepth = ++depth > config.maxDepth;
  const min = config.min;
  if (config.callToJSON && !hitMaxDepth && val.toJSON && typeof val.toJSON === 'function' && !hasCalledToJSON) {
    return printer(val.toJSON(), config, indentation, depth, refs, true);
  }
  const toStringed = toString.call(val);
  if (toStringed === '[object Arguments]') {
    return hitMaxDepth
      ? '[Arguments]'
      : `${min ? '' : 'Arguments '}[${printListItems(val, config, indentation, depth, refs, printer)}]`;
  }
  if (ARRAY_TYPES.has(toStringed)) {
    return hitMaxDepth
      ? `[${val.constructor.name}]`
      : `${
          min
            ? ''
            : !config.printBasicPrototype && val.constructor.name === 'Array'
              ? ''
              : `${val.constructor.name} `
        }[${printListItems(val, config, indentation, depth, refs, printer)}]`;
  }
  if (toStringed === '[object Map]') {
    return hitMaxDepth
      ? '[Map]'
      : `Map {${printIteratorEntries(val.entries(), config, indentation, depth, refs, printer, ' => ')}}`;
  }
  if (toStringed === '[object Set]') {
    return hitMaxDepth
      ? '[Set]'
      : `Set {${printIteratorValues(val.values(), config, indentation, depth, refs, printer)}}`;
  }
  return hitMaxDepth || isWindow(val)
    ? `[${getConstructorName(val)}]`
    : `${
        min
          ? ''
          : !config.printBasicPrototype && getConstructorName(val) === 'Object'
            ? ''
            : `${getConstructorName(val)} `
      }{${printObjectProperties(val, config, indentation, depth, refs, printer)}}`;
}

const isNewPlugin = plugin => plugin.serialize != null;

function printPlugin(plugin, val, config, indentation, depth, refs) {
  let printed;
  try {
    printed = isNewPlugin(plugin)
      ? plugin.serialize(val, config, indentation, depth, refs, printer)
      : plugin.print(
          val,
          valChild => printer(valChild, config, indentation, depth, refs),
          str => {
            const indentationNext = indentation + config.indent;
            return indentationNext + str.replaceAll(NEWLINE_REGEXP, `\n${indentationNext}`);
          },
          {edgeSpacing: config.spacingOuter, min: config.min, spacing: config.spacingInner},
          config.colors,
        );
  } catch (error) {
    throw new PrettyFormatPluginError(error.message, error.stack);
  }
  if (typeof printed !== 'string') {
    throw new TypeError(
      `pretty-format: Plugin must return type "string" but instead returned "${typeof printed}".`,
    );
  }
  return printed;
}

function findPlugin(plugins, val) {
  for (const plugin of plugins) {
    try {
      if (plugin.test(val)) return plugin;
    } catch (error) {
      throw new PrettyFormatPluginError(error.message, error.stack);
    }
  }
  return null;
}

function printer(val, config, indentation, depth, refs, hasCalledToJSON) {
  const plugin = findPlugin(config.plugins, val);
  if (plugin !== null) return printPlugin(plugin, val, config, indentation, depth, refs);
  const basicResult = printBasicValue(val, config.printFunctionName, config.escapeRegex, config.escapeString);
  if (basicResult !== null) return basicResult;
  return printComplexValue(val, config, indentation, depth, refs, hasCalledToJSON);
}

const DEFAULT_THEME = {comment: 'gray', content: 'reset', prop: 'yellow', tag: 'cyan', value: 'green'};

const DEFAULT_OPTIONS = {
  callToJSON: true,
  compareKeys: undefined,
  escapeRegex: false,
  escapeString: true,
  highlight: false,
  indent: 2,
  maxDepth: Number.POSITIVE_INFINITY,
  maxWidth: Number.POSITIVE_INFINITY,
  min: false,
  plugins: [],
  printBasicPrototype: true,
  printFunctionName: true,
  theme: DEFAULT_THEME,
};

function validateOptions(options) {
  for (const key of Object.keys(options)) {
    if (!Object.prototype.hasOwnProperty.call(DEFAULT_OPTIONS, key)) {
      throw new Error(`pretty-format: Unknown option "${key}".`);
    }
  }
  if (options.min && options.indent !== undefined && options.indent !== 0) {
    throw new Error('pretty-format: Options "min" and "indent" cannot be used together.');
  }
  if (options.theme !== undefined) {
    if (options.theme === null) throw new Error('pretty-format: Option "theme" must not be null.');
    if (typeof options.theme !== 'object') {
      throw new TypeError(
        `pretty-format: Option "theme" must be of type "object" but instead received "${typeof options.theme}".`,
      );
    }
  }
}

const NO_COLOR = {close: '', open: ''};
const getColorsEmpty = () => ({comment: NO_COLOR, content: NO_COLOR, prop: NO_COLOR, tag: NO_COLOR, value: NO_COLOR});

const createIndent = indent => Array.from({length: indent + 1}).join(' ');

const getConfig = options => ({
  callToJSON: options?.callToJSON ?? DEFAULT_OPTIONS.callToJSON,
  colors: getColorsEmpty(),
  compareKeys:
    typeof options?.compareKeys === 'function' || options?.compareKeys === null
      ? options.compareKeys
      : DEFAULT_OPTIONS.compareKeys,
  escapeRegex: options?.escapeRegex ?? DEFAULT_OPTIONS.escapeRegex,
  escapeString: options?.escapeString ?? DEFAULT_OPTIONS.escapeString,
  indent: options?.min ? '' : createIndent(options?.indent ?? DEFAULT_OPTIONS.indent),
  maxDepth: options?.maxDepth ?? DEFAULT_OPTIONS.maxDepth,
  maxWidth: options?.maxWidth ?? DEFAULT_OPTIONS.maxWidth,
  min: options?.min ?? DEFAULT_OPTIONS.min,
  plugins: options?.plugins ?? DEFAULT_OPTIONS.plugins,
  printBasicPrototype: options?.printBasicPrototype ?? true,
  printFunctionName: options?.printFunctionName ?? DEFAULT_OPTIONS.printFunctionName,
  spacingInner: options?.min ? ' ' : '\n',
  spacingOuter: options?.min ? '' : '\n',
});

function format(val, options) {
  if (options) {
    validateOptions(options);
    if (options.plugins) {
      const plugin = findPlugin(options.plugins, val);
      if (plugin !== null) return printPlugin(plugin, val, getConfig(options), '', 0, []);
    }
  }
  const basicResult = printBasicValue(
    val,
    options?.printFunctionName ?? DEFAULT_OPTIONS.printFunctionName,
    options?.escapeRegex ?? DEFAULT_OPTIONS.escapeRegex,
    options?.escapeString ?? DEFAULT_OPTIONS.escapeString,
  );
  if (basicResult !== null) return basicResult;
  return printComplexValue(val, getConfig(options), '', 0, []);
}

// --- plugins ---------------------------------------------------------------

const asymmetricMatcherSymbol = Symbol.for('jest.asymmetricMatcher');

const AsymmetricMatcher = {
  serialize(val, config, indentation, depth, refs, printer) {
    const stringedValue = val.toString();
    if (stringedValue === 'ArrayContaining' || stringedValue === 'ArrayNotContaining') {
      if (++depth > config.maxDepth) return `[${stringedValue}]`;
      return `${stringedValue} [${printListItems(val.sample, config, indentation, depth, refs, printer)}]`;
    }
    if (stringedValue === 'ObjectContaining' || stringedValue === 'ObjectNotContaining') {
      if (++depth > config.maxDepth) return `[${stringedValue}]`;
      return `${stringedValue} {${printObjectProperties(val.sample, config, indentation, depth, refs, printer)}}`;
    }
    if (
      stringedValue === 'StringMatching' ||
      stringedValue === 'StringNotMatching' ||
      stringedValue === 'StringContaining' ||
      stringedValue === 'StringNotContaining'
    ) {
      return `${stringedValue} ${printer(val.sample, config, indentation, depth, refs)}`;
    }
    if (stringedValue === 'ArrayOf' || stringedValue === 'NotArrayOf') {
      if (++depth > config.maxDepth) return `[${stringedValue}]`;
      return `${stringedValue} ${printer(val.sample, config, indentation, depth, refs)}`;
    }
    if (typeof val.toAsymmetricMatcher !== 'function') {
      throw new TypeError(
        `Asymmetric matcher ${val.constructor.name} does not implement toAsymmetricMatcher()`,
      );
    }
    return val.toAsymmetricMatcher();
  },
  test: val => val && val.$$typeof === asymmetricMatcherSymbol,
};

const escapeHTML = str => str.replaceAll('<', '&lt;').replaceAll('>', '&gt;');

function printProps(keys, props, config, indentation, depth, refs, printer) {
  const indentationNext = indentation + config.indent;
  const colors = config.colors;
  return keys
    .map(key => {
      const value = props[key];
      let printed = printer(value, config, indentationNext, depth, refs);
      if (typeof value !== 'string') {
        if (printed.includes('\n')) {
          printed = config.spacingOuter + indentationNext + printed + config.spacingOuter + indentation;
        }
        printed = `{${printed}}`;
      }
      return `${config.spacingInner + indentation + colors.prop.open + key + colors.prop.close}=${
        colors.value.open
      }${printed}${colors.value.close}`;
    })
    .join('');
}

function printChildren(children, config, indentation, depth, refs, printer) {
  return children
    .map(
      child =>
        config.spacingOuter +
        indentation +
        (typeof child === 'string' ? printText(child, config) : printer(child, config, indentation, depth, refs)),
    )
    .join('');
}

const printText = (text, config) =>
  config.colors.content.open + escapeHTML(text) + config.colors.content.close;

const printComment = (comment, config) =>
  `${config.colors.comment.open}<!--${escapeHTML(comment)}-->${config.colors.comment.close}`;

function printElement(type, printedProps, printedChildren, config, indentation) {
  const tagColor = config.colors.tag;
  return `${tagColor.open}<${type}${
    printedProps && tagColor.close + printedProps + config.spacingOuter + indentation + tagColor.open
  }${
    printedChildren
      ? `>${tagColor.close}${printedChildren}${config.spacingOuter}${indentation}${tagColor.open}</${type}`
      : `${printedProps && !config.min ? '' : ' '}/`
  }>${tagColor.close}`;
}

const printElementAsLeaf = (type, config) =>
  `${config.colors.tag.open}<${type}${config.colors.tag.close} …${config.colors.tag.open} />${config.colors.tag.close}`;

// react-is, for React 18 and 19 at once.
const REACT_ELEMENT = Symbol.for('react.element');
const REACT_TRANSITIONAL_ELEMENT = Symbol.for('react.transitional.element');
const REACT = {
  fragment: Symbol.for('react.fragment'),
  suspense: Symbol.for('react.suspense'),
  provider: Symbol.for('react.provider'),
  context: Symbol.for('react.context'),
  consumer: Symbol.for('react.consumer'),
  forwardRef: Symbol.for('react.forward_ref'),
  memo: Symbol.for('react.memo'),
};
const isReactElement = val =>
  typeof val === 'object' &&
  val !== null &&
  (val.$$typeof === REACT_ELEMENT || val.$$typeof === REACT_TRANSITIONAL_ELEMENT);
const typeOfType = element => element.type && element.type.$$typeof;
const isFragment = el => isReactElement(el) && el.type === REACT.fragment;
const isSuspense = el => isReactElement(el) && el.type === REACT.suspense;
// React 18: <Context.Provider> is react.provider and <Context.Consumer> is
// react.context. React 19: the context itself is the provider (react.context)
// and the consumer is react.consumer.
const isContextProvider = el =>
  isReactElement(el) &&
  (el.$$typeof === REACT_ELEMENT
    ? typeOfType(el) === REACT.provider
    : typeOfType(el) === REACT.context);
const isContextConsumer = el =>
  isReactElement(el) &&
  (el.$$typeof === REACT_ELEMENT
    ? typeOfType(el) === REACT.context
    : typeOfType(el) === REACT.consumer);
const isForwardRef = el => isReactElement(el) && typeOfType(el) === REACT.forwardRef;
const isMemo = el => isReactElement(el) && typeOfType(el) === REACT.memo;

function getChildren(arg, children = []) {
  if (Array.isArray(arg)) {
    for (const item of arg) getChildren(item, children);
  } else if (arg != null && arg !== false && arg !== '') {
    children.push(arg);
  }
  return children;
}

function getType(element) {
  const type = element.type;
  if (typeof type === 'string') return type;
  if (typeof type === 'function') return type.displayName || type.name || 'Unknown';
  if (isFragment(element)) return 'React.Fragment';
  if (isSuspense(element)) return 'React.Suspense';
  if (typeof type === 'object' && type !== null) {
    if (isContextProvider(element)) return 'Context.Provider';
    if (isContextConsumer(element)) return 'Context.Consumer';
    if (isForwardRef(element)) {
      if (type.displayName) return type.displayName;
      const functionName = type.render.displayName || type.render.name || '';
      return functionName === '' ? 'ForwardRef' : `ForwardRef(${functionName})`;
    }
    if (isMemo(element)) {
      const functionName = type.displayName || type.type.displayName || type.type.name || '';
      return functionName === '' ? 'Memo' : `Memo(${functionName})`;
    }
  }
  return 'UNDEFINED';
}

const ReactElement = {
  serialize(element, config, indentation, depth, refs, printer) {
    const props = element.props;
    const keys = Object.keys(props)
      .filter(key => key !== 'children' && props[key] !== undefined)
      .sort();
    return ++depth > config.maxDepth
      ? printElementAsLeaf(getType(element), config)
      : printElement(
          getType(element),
          printProps(keys, props, config, indentation + config.indent, depth, refs, printer),
          printChildren(getChildren(props.children), config, indentation + config.indent, depth, refs, printer),
          config,
          indentation,
        );
  },
  test: val => val != null && isReactElement(val),
};

const reactTestSymbol = Symbol.for('react.test.json');

const ReactTestComponent = {
  serialize(object, config, indentation, depth, refs, printer) {
    const props = object.props;
    const keys = props
      ? Object.keys(props)
          .filter(key => props[key] !== undefined)
          .sort()
      : [];
    return ++depth > config.maxDepth
      ? printElementAsLeaf(object.type, config)
      : printElement(
          object.type,
          props ? printProps(keys, props, config, indentation + config.indent, depth, refs, printer) : '',
          object.children
            ? printChildren(object.children, config, indentation + config.indent, depth, refs, printer)
            : '',
          config,
          indentation,
        );
  },
  test: val => val && val.$$typeof === reactTestSymbol,
};

const ELEMENT_NODE = 1;
const TEXT_NODE = 3;
const COMMENT_NODE = 8;
const FRAGMENT_NODE = 11;
const ELEMENT_REGEXP = /^((HTML|SVG)\w*)?Element$/;

const testHasAttribute = val => {
  try {
    return typeof val.hasAttribute === 'function' && val.hasAttribute('is');
  } catch {
    return false;
  }
};
const isCustomElement = val => {
  const tagName = val?.tagName;
  return (typeof tagName === 'string' && tagName.includes('-')) || testHasAttribute(val);
};
const testNode = val => {
  const constructorName = val.constructor.name;
  const {nodeType} = val;
  return (
    (nodeType === ELEMENT_NODE && (ELEMENT_REGEXP.test(constructorName) || isCustomElement(val))) ||
    (nodeType === TEXT_NODE && constructorName === 'Text') ||
    (nodeType === COMMENT_NODE && constructorName === 'Comment') ||
    (nodeType === FRAGMENT_NODE && constructorName === 'DocumentFragment')
  );
};

const DOMElement = {
  serialize(node, config, indentation, depth, refs, printer) {
    if (node.nodeType === TEXT_NODE) return printText(node.data, config);
    if (node.nodeType === COMMENT_NODE) return printComment(node.data, config);
    const fragment = node.nodeType === FRAGMENT_NODE;
    const type = fragment ? 'DocumentFragment' : node.tagName.toLowerCase();
    if (++depth > config.maxDepth) return printElementAsLeaf(type, config);
    return printElement(
      type,
      printProps(
        fragment ? [] : Array.from(node.attributes, attr => attr.name).sort(),
        fragment
          ? {}
          : [...node.attributes].reduce((props, attribute) => {
              props[attribute.name] = attribute.value;
              return props;
            }, {}),
        config,
        indentation + config.indent,
        depth,
        refs,
        printer,
      ),
      printChildren(
        Array.prototype.slice.call(node.childNodes || node.children),
        config,
        indentation + config.indent,
        depth,
        refs,
        printer,
      ),
      config,
      indentation,
    );
  },
  test: val => (val?.constructor?.name || isCustomElement(val)) && testNode(val),
};

const OBJECT_NAMES = new Set(['DOMStringMap', 'NamedNodeMap']);
const ARRAY_REGEXP = /^(HTML\w*Collection|NodeList)$/;

const DOMCollection = {
  serialize(collection, config, indentation, depth, refs, printer) {
    const name = collection.constructor.name;
    if (++depth > config.maxDepth) return `[${name}]`;
    return (
      (config.min ? '' : `${name} `) +
      (OBJECT_NAMES.has(name)
        ? `{${printObjectProperties(
            name === 'NamedNodeMap'
              ? [...collection].reduce((props, attribute) => {
                  props[attribute.name] = attribute.value;
                  return props;
                }, {})
              : {...collection},
            config,
            indentation,
            depth,
            refs,
            printer,
          )}}`
        : `[${printListItems([...collection], config, indentation, depth, refs, printer)}]`)
    );
  },
  test: val =>
    val && val.constructor && !!val.constructor.name &&
    (OBJECT_NAMES.has(val.constructor.name) || ARRAY_REGEXP.test(val.constructor.name)),
};

module.exports = {
  format,
  DEFAULT_OPTIONS,
  plugins: {AsymmetricMatcher, DOMCollection, DOMElement, ReactElement, ReactTestComponent},
  printListItems,
  printObjectProperties,
};
