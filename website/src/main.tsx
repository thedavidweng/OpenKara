import React from "react";
import ReactDOM from "react-dom/client";
import "@/styles/globals.css";
import { LandingPage } from "./LandingPage";
import "./site.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <LandingPage />
  </React.StrictMode>,
);
