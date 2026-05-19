/* @refresh reload */
import { render } from "solid-js/web";
import { Router, Route } from "@solidjs/router";
import App from "./App";
import FloatingPopup from "./components/FloatingPopup";
import "./styles/globals.css";

render(
  () => (
    <Router>
      <Route path="/" component={App} />
      <Route path="/popup" component={FloatingPopup} />
    </Router>
  ),
  document.getElementById("root") as HTMLElement
);
