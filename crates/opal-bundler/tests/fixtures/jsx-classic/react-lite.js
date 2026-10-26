export default {
  createElement(type, props, ...children) {
    return typeof type === "function" ? type({ ...props, children }) : { type, props, children };
  },
  Fragment: "Fragment",
};
