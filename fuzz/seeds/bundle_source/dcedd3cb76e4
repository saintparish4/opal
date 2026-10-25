// Rule 14: a define is the program run where the path has that value.
console.log(process.env.NODE_ENV, typeof process.env.NODE_ENV);
const process2 = { env: { NODE_ENV: "local binding wins" } };
{
  const process = process2;
  console.log(process.env.NODE_ENV);
}
