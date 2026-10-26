// JSX with the classic runtime (tsconfig "jsx": "react"): calls
// React.createElement, which must be in scope.
import React from "./react-lite.js";
const List = ({ items }) => <ul>{items.map((item) => <li key={item}>{item}</li>)}</ul>;
console.log(JSON.stringify(<List items={["a", "b"]} />));
