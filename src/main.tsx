import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { api, isDesktop } from "./api";
import "./style.css";
createRoot(document.getElementById("root")!).render(
  isDesktop ? (
    <App api={api} />
  ) : (
    <div className="empty">
      <h1>墨卷</h1>
      <p>请从桌面应用打开。本页面不会用浏览器存储冒充 SQLite 书库。</p>
    </div>
  ),
);
