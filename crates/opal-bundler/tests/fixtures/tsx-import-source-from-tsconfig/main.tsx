// The tsconfig names preact as the JSX import source, so the compiler
// imports preact/jsx-runtime, and the graph has to have that edge.
const tree = <div className="note">{1 + 1}</div>;
console.log(JSON.stringify(tree));
