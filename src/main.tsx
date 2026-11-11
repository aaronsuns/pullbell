import React from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";
import { Queue } from "./views/Queue";
import { MyPrs } from "./views/MyPrs";
import { SettingsView } from "./views/Settings";
import { Welcome } from "./views/Welcome";
import { About } from "./views/About";

const views: Record<string, () => React.JSX.Element> = {
  queue: Queue,
  myprs: MyPrs,
  settings: SettingsView,
  welcome: Welcome,
  about: About,
};

const View = views[window.location.hash.replace("#", "")] ?? About;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <View />
  </React.StrictMode>,
);
