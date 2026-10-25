// JSX with the automatic runtime (tsconfig "jsx": "react-jsx"): the
// compiler adds an import of react/jsx-runtime, which the graph resolves
// (opal-core's jsx_runtime option) and the bundle includes.
import { Card } from "./card.tsx";

const tree = (
  <>
    <Card title="one" />
    <Card title="two">
      <span className="note">{1 + 1}</span>
    </Card>
  </>
);
console.log(JSON.stringify(tree));
