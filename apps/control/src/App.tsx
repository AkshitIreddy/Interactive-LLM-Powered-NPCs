import "./product.css";
import "./review.css";
import "./cyberpunk.css";
import { ProductConsole } from "./ProductConsole";
import { ThemeSpecimen } from "./ThemeSpecimen";

export function App() {
  if (
    new URLSearchParams(window.location.search).get("themeSpecimen") === "1"
  ) {
    return <ThemeSpecimen />;
  }
  return <ProductConsole />;
}
