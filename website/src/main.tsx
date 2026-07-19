import React from "react";
import ReactDOM from "react-dom/client";
import "@/styles/globals.css";
import { DocumentPage } from "./DocumentPage";
import { getDocumentRoute } from "./document-route";
import { LandingPage } from "./LandingPage";
import "./site.css";

const documentRoute = getDocumentRoute(window.location.pathname);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {documentRoute ? <DocumentPage {...documentRoute} /> : <LandingPage />}
  </React.StrictMode>,
);
