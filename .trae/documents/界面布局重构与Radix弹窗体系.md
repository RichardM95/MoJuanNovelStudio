# 界面布局重构与 Radix 弹窗体系

> 本轮修订：整体方向不变，按“方向对，只改细节”的反馈补充了 3.12 视觉细节规范、3.13 原截图对照验收，以及导入弹窗双栏示意；实施顺序把导入弹窗提前，先交付最直观的截图修复。

## 1. 目标

- 修复截图所示导入弹窗及全局布局问题，让书架、编辑器、全部弹窗在 900×600 到 1280×850 的窗口下都保持清晰层次与稳定结构。
- 引入 Radix UI 无样式 Dialog，统一弹窗的焦点陷阱、Esc 关闭、焦点恢复和遮罩行为，替换 15+ 个面板各自手写的弹窗逻辑。
- 建立一套设计令牌与共享布局骨架，消除“每个面板一套弹窗尺寸/滚动策略”的碎片化。
- 保持墨青 + 米纸国风；不改业务逻辑、接口调用、按钮文案、字段标签与现有测试依赖的可访问名称。

## 2. 现状分析

### 2.1 当前结构

- 应用外壳：[App.tsx](file:///e:/Program%20Files%20(x86)/MoJuanNovelStudio/src/App.tsx#L512-L587) 使用固定 `.rail`（208px，`position:fixed`）+ `main` 的 `margin-left: 208px` + `height:100vh`。
- 编辑器：[App.tsx](file:///e:/Program%20Files%20(x86)/MoJuanNovelStudio/src/App.tsx#L793-L1082) 为 `editor-top`（64px）+ `editor-tools`（约 43px）+ `paper-toolbar`（50px）+ 正文 + `footer`（48px），左侧 `.directory` 268px，正文列最大 720px。
- 弹窗：项目里同时存在两套手写骨架——`.modal-backdrop + .modal` 与 `.review-backdrop + .review-panel/.materials-panel/.backup-panel`，每个面板自行实现 header、滚动、Escape、Tab 陷阱与焦点恢复。

### 2.2 问题根因

1. **裸 `main` 选择器污染嵌套 `<main>`**：[style.css](file:///e:/Program%20Files%20(x86)/MoJuanNovelStudio/src/style.css#L310-L321) 的 `main { margin-left:208px; height:100vh; overflow-y:auto }` 会命中弹窗内部的 `<main>`。`TasksPanel` 因此内联写了 `style={{ marginLeft: 0 }}`（[TasksPanel.tsx](file:///e:/Program%20Files%20(x86)/MoJuanNovelStudio/src/TasksPanel.tsx#L365)），`MaterialsPanel` 的 `.materials-layout > main` 没有完全抵消，导致双栏弹窗内部被额外推挤、出现错误纵向滚动。
2. **弹窗后代选择器全局化**：[style.css](file:///e:/Program%20Files%20(x86)/MoJuanNovelStudio/src/style.css#L1252-L1321) 的 `.modal h2`、`.modal p`、`.modal label` 会改写弹窗内所有标题、段落和表单行；各面板只能靠零散覆盖修正，布局节奏不稳定。
3. **两套弹窗骨架不一致**：`.modal-backdrop`（z-index 20）与 `.review-backdrop`（z-index 30）尺寸、遮罩、滚动策略不同；`.modal` 没有基础 `max-height`，`.review-panel/.materials-panel/.backup-panel` 又各自设 `max-height/overflow`。长内容时标题随内容滚走、主要操作落在折叠区。
4. **导入弹窗是单列长滚动**：[ImportPanel.tsx](file:///e:/Program%20Files%20(x86)/MoJuanNovelStudio/src/ImportPanel.tsx#L303-L472) 把配置、超长分章说明、元信息、章节列表、正文预览、主操作、二次确认全部纵向堆叠；在 1024×678 下首屏只能看到配置与说明，主要操作被推到下方，形成截图中“页面很挤、重点不明”的感受。
5. **番茄工作区完全没有样式**：`FanqieWorkspace.tsx` 使用 11 个 `fanqie-*` 类，但 `style.css` 中没有任何对应规则（已用脚本核对），该页面目前是浏览器默认排版。
6. **CSS 后置重复覆盖**：[style.css](file:///e:/Program%20Files%20(x86)/MoJuanNovelStudio/src/style.css#L1907-L1978) 又重复定义 `.review-backdrop/.review-panel/.materials-panel/.backup-panel`，同一属性在文件不同位置取值，后续维护容易改错。
7. **编辑器垂直空间紧张**：64 + 44 + 50 + 48 ≈ 206px 固定 chrome；900×600 最小窗口下正文只剩约 394px，正文列又被 `clamp(24px,6vw,110px)` 左右内边距压缩到约 400px。

### 2.3 测试约束

- 40 个测试文件大量使用 `getByRole("dialog", { name })`、按钮名称、`getByLabelText`、`aria-pressed`、Escape、焦点陷阱与焦点恢复断言。
- 必须保留的语义包括但不限于：
  - 弹窗名称：`导入TXT新作品`、`确认操作`、`章节写作约束`、`全书伏笔`、`改稿与历史`、`创作资料库`、`备份与恢复`、`检查任务` 等。
  - 关闭按钮名称：`关闭导入面板`、`关闭备份面板`、`关闭资料库`、`关闭改稿面板`、`关闭全书伏笔`、`关闭写作约束`、`关闭拆章面板` 等。
  - 导入流程按钮：`选择TXT文件`、`更新预览`、`导入为新作品`、`返回预览`、`确认导入`、`重新加载书架`；以及 `仅UTF-8…最多16 MiB…单章最多10 MiB` 说明文本仍在 DOM 中。
  - 表单标签：`分章模式`、`书名`、`作者`、`导出范围`、`名称`、`输入名称确认` 等。
- 不得把确认弹窗改成 Radix `AlertDialog`：它渲染 `role="alertdialog"`，会让现有 `getByRole("dialog", { name: "确认操作" })` 失效。

## 3. 设计方案

### 3.1 新增依赖

- 只新增 `@radix-ui/react-dialog`（当前最新 1.3.x，兼容 React 19）。
- 不引入 Tailwind、Ant Design、MUI、Radix Tabs/Select/AlertDialog：
  - 原生 `<select>/<input>/<textarea>` 被大量测试用 label 直接查询，替换成 Radix Select 会牵连 20+ 表单且无布局收益。
  - 现有分类标签使用 `aria-pressed`（如资料分类、伏笔卡列表），改 Radix Tabs 会改变语义。
  - 确认弹窗必须保持 `role="dialog"`。
- 安装命令：`yarn add @radix-ui/react-dialog`；更新 `package.json` 与 `yarn.lock`。若 registry 不可达，停止并报告，不静默改用其他方案。

### 3.2 设计令牌（写入新 `src/styles/ui.css` 的 `:root`）

```css
--space-1: 4px;
--space-2: 8px;
--space-3: 12px;
--space-4: 16px;
--space-5: 20px;
--space-6: 24px;
--space-7: 32px;
--space-8: 40px;
--radius-sm: 6px;
--radius-md: 10px;
--radius-lg: 14px;
--radius-xl: 18px;
--rail-w: 208px;
--header-h: 56px;
--toolbar-h: 44px;
--z-rail: 10;
--z-dialog: 60;
--z-toast: 70;
```

- 颜色继续复用现有 `--ink/--ink-soft/--muted/--line/--bg/--paper/--surface/--accent/--gold/--danger`，只补充 `--surface-muted:#f7f5ef`、`--surface-raised:#fffdfa`。
- 字号：页面标题 30px 衬线、区块标题 16px、正文 14px、辅助 12.5px、元信息 11.5px。
- 控件高度沿用 `--control-h-sm:28px / --control-h:34px / --control-h-lg:38px`。

### 3.3 应用外壳

- `src/styles/ui.css` 中新增：
  - `html, body, #root { height:100%; }`、`body { overflow:hidden; }`。
  - `.app { display:grid; grid-template-columns:var(--rail-w) minmax(0,1fr); height:100dvh; overflow:hidden; }`。
  - `.rail { position:relative; inset:auto; height:100%; overflow:auto; }`，删除 `position:fixed`。
  - `.shell-main { min-width:0; height:100%; min-height:0; margin-left:0; overflow:hidden; }`。
  - `main` 元素选择器不再写布局；`App.tsx` 给主区域加 `className="shell-main …"`。
  - `.shelf-main { overflow-y:auto; }`；`.editor-main { display:flex; flex-direction:column; }`。
- 侧栏：
  - 品牌区下留 28px；导航按钮统一 40px 行高、圆角 8px、图标 18px。
  - `.rail-bottom` 增加分隔线与顶部小标题间距；`.local-dot` 改为胶囊状态条（浅色底、6px 圆点）。
  - 激活项加 `aria-current="page"`（保留 `.active` 类），不改变按钮名称。
  - 断点：≤1000px 侧栏 184px；≤900px 168px；≤720px 保持现有 64px 图标栏。
  - 所有 `main { margin-left:… }` 的媒体查询同步删除。

### 3.4 书架

- `.shelf`：`max-width:1240px; padding:44px clamp(24px,4vw,56px) 72px;`
- `.shelf-header`：基线对齐、标题 30px、说明 13px；操作区按钮 36px，主按钮 38px。
- `.search`：宽 360px、高 40px、`margin-bottom:28px`，与书架标题左对齐。
- `.books`：`grid-template-columns:repeat(auto-fill,minmax(172px,1fr)); gap:32px 26px;`
- `.book`：去掉 `max-width:232px`；封面保持 0.72 比例与墨色渐变。
- `.book-info`：改为白色信息卡——`background:var(--surface); border:1px solid var(--line); border-radius:var(--radius-md); padding:12px 14px; margin-top:10px; box-shadow:var(--shadow-sm);`；书名 14px、作者/字数 12px；`.book-actions` 放在卡片底部。
- 空状态 `.empty` 使用虚线卡片容器，保持现有文案。

### 3.5 编辑器

- `.workspace`：`display:grid; grid-template-rows:auto auto minmax(0,1fr);` 背景使用 `var(--paper)`。
- `.editor-top`：高度 `--header-h`（56px），左右内边距 20px；标题 17px，作者 11px；保存为次按钮，导出为主按钮。
- `.editor-tools`：高度 44px；按钮 32px 高、圆角 8px、白底细边；按功能插入 `<span className="tool-divider" aria-hidden="true" />`（写作 / 编辑 / 资料 / 审阅 / 发布 五组），分隔线 1px×18px、颜色 `--line`；横向溢出仍可滚动。
- `.writing-layout`：`min-height:0;`
- `.directory`：`width:clamp(228px,22vw,280px); padding:14px 10px 20px;` 背景 `--surface-muted`；卷/章行保持现有紧凑样式，仅统一行高与 hover。
- `.manuscript`：`padding:0 clamp(20px,4vw,64px);` 内部所有块 `max-width:760px`。
- `.paper-toolbar`：高度 46px；删除按钮把危险色 hover 保留；select 高 28px。
- `.chapter-heading`：28px 衬线，上下留白 22px/8px。
- `.manuscript textarea`：`line-height:1.95`，上下 padding 10px/24px。
- `.manuscript footer`：高度 44px，分隔线 `--line`；保存状态用绿色胶囊。
- 断点：≤1000px 目录 176px、正文内边距 20px；≤900px 目录 168px；≤720px 目录 160px。这样 900px 窗口下正文列仍可保持约 510px。

### 3.6 共享 Dialog 组件

新增 `src/ui/Dialog.tsx`，基于 `@radix-ui/react-dialog`：

```tsx
type DialogSize = "sm" | "md" | "lg" | "xl" | "full";

type DialogProps = {
  title: string;
  subtitle?: string;
  label?: string;              // 默认等于 title，用于 getByRole("dialog", { name })
  closeLabel: string;          // 必须与现有 aria-label 完全一致
  onClose: () => void;         // 面板现有的 gated close
  dismissible?: boolean;       // false 时禁用 Esc 与关闭按钮
  size?: DialogSize;
  showClose?: boolean;         // 默认 true；确认弹窗可保留
  headerActions?: ReactNode;
  footer?: ReactNode;
  className?: string;
  bodyClassName?: string;
  initialFocusRef?: React.RefObject<HTMLElement>;
  children: ReactNode;
};
```

行为与 DOM：

- `Dialog.Root open` + `Dialog.Portal` + `Dialog.Overlay` + `Dialog.Content`。
- `Content` 加 `className="ui-dialog ui-dialog-{size} {className}"`，`aria-label={label ?? title}`；Radix 自动提供 `role="dialog"` 与 `aria-modal="true"`。
- 头部：`Dialog.Title`（渲染 `h2.ui-dialog-title`）+ 可选 `Dialog.Description` + `headerActions` + `Dialog.Close asChild` 的关闭按钮。
- 关闭按钮：`<button type="button" className="ui-dialog-close" aria-label={closeLabel} disabled={!dismissible}><X size={18}/></button>`；保留每个面板原有名称，不依赖可见文字“关闭/×”。
- `onOpenChange(false)` 一律调用 `onClose()`（面板内部已有 busy/locked 判断）；`dismissible=false` 时 `onEscapeKeyDown` 阻止默认行为，关闭按钮 disabled。
- `onPointerDownOutside` 与 `onInteractOutside` 全部 `preventDefault()`，保持现有“点击遮罩不关闭”的行为。
- 打开时：若传入 `initialFocusRef` 则聚焦它；否则聚焦 `.ui-dialog-body` 中第一个可用控件（button/input/select/textarea），与现有各面板 `controls()[0]?.focus()` 一致。Radix FocusScope 负责 Tab 循环、外部焦点拉回与卸载后焦点恢复。
- `body` 渲染为 `.ui-dialog-body`，`footer` 渲染为 `.ui-dialog-footer`。
- 含 `<form>` 的小弹窗（App 新增/改名、RecycleBin 永久删除）：表单放在 body 内并设置固定 `id`，footer 的提交按钮用 `type="submit" form="<id>"` 关联；`showClose={false}`，保留 footer 的 `取消` 按钮。
- 迁移面板后删除各面板手写的 `focusin`、`keydown`（Escape/Tab）监听与 `previous.focus()`；保留业务清理（丢弃导入预览、pending 复位、定时器等）。

共享样式：

```css
.ui-dialog {
  position: fixed;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  z-index: var(--z-dialog);
  display: grid;
  grid-template-rows: auto minmax(0, 1fr) auto;
  width: var(--dialog-w);
  max-width: calc(100vw - 40px);
  max-height: min(88dvh, 960px);
  background: var(--surface);
  border: 1px solid #ffffff6b;
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-lg);
  overflow: hidden;
}
.ui-dialog-sm { --dialog-w: 440px; }
.ui-dialog-md { --dialog-w: 680px; }
.ui-dialog-lg { --dialog-w: 920px; }
.ui-dialog-xl { --dialog-w: 1100px; }
.ui-dialog-full { --dialog-w: min(1280px, 96vw); }
.ui-dialog-header { display:flex; align-items:flex-start; gap:12px; padding:20px 24px 16px; border-bottom:1px solid var(--line); background:linear-gradient(180deg,#fffefb,#fbf9f3); }
.ui-dialog-title { margin:0; font:700 20px/1.4 var(--font-serif); letter-spacing:1px; }
.ui-dialog-subtitle { margin:4px 0 0; font-size:12.5px; line-height:1.6; color:var(--muted); }
.ui-dialog-body { min-height:0; overflow:auto; overscroll-behavior:contain; padding:20px 24px 24px; }
.ui-dialog-body-flush { padding:0; }
.ui-dialog-footer { display:flex; align-items:center; justify-content:flex-end; gap:10px; padding:14px 24px; border-top:1px solid var(--line); background:#fcfaf5; }
.ui-overlay { position:fixed; inset:0; z-index:59; background:#16282e80; backdrop-filter:blur(3px); animation:fade-in .16s ease; }
```

- 不再给 Content 加旧 `modal` / `review-panel` 类，避免旧后代选择器命中；各面板保留自己的业务类（如 `txt-import`）仅用于内部样式。
- 旧 `.modal`、`.modal-backdrop`、`.review-backdrop`、`.review-panel` 根布局规则在全部迁移后删除。

### 3.7 弹窗尺寸与布局分配

| 组件 | size | 头部/布局 | 说明 |
| --- | --- | --- | --- |
| ImportPanel | lg（920） | 双栏（设置 300px + 预览） | 固定头尾，确认态替换 footer |
| ProjectImportPanel | lg（920） | 单栏分区卡片 | 文件列表/字段网格/预览分区 |
| ExportPanel | md（680） | 单栏表单 + 预览卡 | 保存TXT 放 footer 主操作 |
| SplitPanel | md（680） | 上下两段预览 | 确认按钮在 footer |
| ReplacePanel | xl（1100） | 表单行 + 左章节列表 / 右结果 | 结果区独立滚动 |
| SearchPanel | lg（920） | 搜索栏 + 结果卡片列表 | 结果列表滚动 |
| AgentAccessPanel | md（680） | 状态卡 + 命令块 | 不再复用 `txt-import` 宽度 |
| BackupPanel | lg（920） | 顶部动作导航 + 分区卡片 | 长内容只在 body 滚动 |
| ReviewPanel | xl（1100） | header + tabs + 内容 | body 滚动；history 子面板保持 |
| MaterialsPanel | xl（1100） | body-flush + 左列表/右编辑 | 移除嵌套 main 的全局 margin |
| TasksPanel | xl（1100） | body-flush + 左记录/右报告 | 删除 `style={{marginLeft:0}}` 补丁 |
| WritingPanel | lg（860） | 单栏长表单，body 滚动 | 保留表单内保存操作 |
| ForeshadowPanel | xl（1100） | body-flush + 左卡列表/右详情 | 关闭名称 `关闭全书伏笔` |
| RecycleBin 永久删除 | sm（440） | 确认表单 | Dialog，非 AlertDialog |
| App 新增/改名表单 | sm（440） | 表单 | 保留 `名称/作者` label 包裹 |
| App 确认操作 | sm（440） | 文本 + 底部操作 | 保留 `aria-label="确认操作"`、按钮 `取消/确认` |

### 3.8 导入弹窗重排（截图重点）

保持 `aria-label="导入TXT新作品"`，结构改为：

- 头部：标题 `导入TXT新作品`，副标题 `始终创建新作品，不覆盖同名作品，原文件不修改。`，右上关闭按钮 `aria-label="关闭导入面板"`。
- Body 两栏：
  - 左栏 `.import-setup`（约 300px）：
    - `.ui-card` 第 1 步“选择文件”：`选择TXT文件` 按钮（全宽）；已选时显示文件名、`source_name`、大小/模式摘要。
    - `.ui-card` 第 2 步“书籍信息”：`分章模式` select、`书名`、`作者` 输入；`更新预览` 按钮。
    - `<details class="import-rules"><summary>分章规则与限制</summary><p>仅UTF-8…</p></details>`：保留原有完整说明文本，测试仍能用 `getByText(/仅UTF-8…/)` 找到。
  - 右栏 `.import-preview`：
    - 空态：图标 + `选择 TXT 文件后在此核对目录与正文。`
    - 有预览时：摘要条（`《书名》 · 作者 · 模式 · N卷、N章 · 正文字节` + `有效至…` 或过期提示）。
    - `.import-chapters` 章节列表卡片（`max-height:360px` 独立滚动，选中行左侧 3px 主色条）。
    - `.import-detail` 正文预览卡（`pre` 最大高度 280px 独立滚动）。
    - 保留错误、过期、`正在读取所选章节…`、`导入结果未确认…` 等提示。
- Footer：
  - 默认：左侧状态文字（有效至/错误/成功），右侧 `导入为新作品` 主按钮。
  - 点击后进入确认态：footer 内显示 `确认创建《…》…？不会覆盖已有作品。` + `返回预览`（ghost）+ `确认导入`（primary）。
  - 成功后：`已导入新作品：N卷、N章。` + `重新加载书架` / 关闭。
- 测试兼容：`导入为新作品` 在确认态可隐藏（测试只点击一次后转确认）；`返回预览`/`确认导入` 名称不变；`关闭导入面板` 在 blocked 时 disabled。

布局示意（920×约 640 的弹窗）：

```text
┌──────────────────────────────────────────────────────────────┐
│ 导入TXT新作品                                          [×]   │
│ 始终创建新作品，不覆盖同名作品，原文件不修改。               │
├───────────────────────────┬──────────────────────────────────┤
│ ① 选择文件                │ 预览                             │
│ ┌───────────────────────┐ │ 《书名》·作者·10卷/600章·4.9MB   │
│ │    选择TXT文件        │ │ 有效至 12:53:49                  │
│ └───────────────────────┘ │ ┌──────────────────────────────┐ │
│ 稿件.txt · 4.9 MB         │ │ 卷一（5章）                  │ │
│                           │ │ 第一章 · 6685字节            │ │
│ ② 书籍信息                │ │ ……                          │ │
│ 分章模式 [按标题分章  v]  │ └──────────────────────────────┘ │
│ 书名 [.................]  │ 正文预览                         │
│ 作者 [.................]  │ ┌──────────────────────────────┐ │
│ [更新预览]                │ │ 但虫群的数量实在太多了……     │ │
│                           │ └──────────────────────────────┘ │
│ ▸ 分章规则与限制          │                                  │
├───────────────────────────┴──────────────────────────────────┤
│ 有效至 12:53:49                          [ 导入为新作品 ]    │
└──────────────────────────────────────────────────────────────┘
```

### 3.9 其他面板内部整理

- 统一使用 `.ui-card`（白底、1px 边、10px 圆角、16px 内边距、标题 13px/600）分组。
- 表单行统一 `.ui-field`：`display:flex; flex-direction:column; gap:6px; font-size:13px;`；不再依赖 `.modal label` 的全局规则。
- 列表行统一 `.ui-list-row`：左对齐、透明底、hover 浅米底、`[aria-pressed="true"]` 浅青底 + 左侧 3px 主色条。
- 危险操作统一 `.danger / .danger-text`；主操作 `.primary`。
- 只调整 JSX 的包裹层与 className，不改事件处理、状态机与文案。

### 3.10 番茄工作区补样式

在 `src/styles/ui.css` 增加：

- `.fanqie-workspace`：`max-width:1080px; margin:0 auto; padding:40px clamp(24px,4vw,56px) 64px; overflow-y:auto; height:100%;`
- `.fanqie-header`：标题 28px 衬线 + 说明，右侧返回/退出按钮。
- `.fanqie-card`：白色卡片、1px 边、12px 圆角、20px 内边距、区块间距 18px。
- `.fanqie-field`：label 13px，input/select/textarea 全宽。
- `.fanqie-volume`：浅米底卡片；`summary` 13px/600；`.fanqie-options` 两列复选网格（≤720px 单列）。
- `.fanqie-mapping`：`repeat(auto-fit,minmax(180px,1fr))` 网格。
- `.fanqie-actions`：按钮组换行，gap 8px。
- `.fanqie-counter`：小号胶囊；`.fanqie-error` 危险色提示条。

### 3.11 CSS 清理

- 新建 `src/styles/ui.css`，在 `main.tsx` 中先导入 `./style.css`、再导入 `./styles/ui.css`，确保新系统覆盖旧规则。
- 从 `style.css` 删除/替换：
  - 裸 `main { margin-left/height/overflow }` 与所有 `main { margin-left:… }` 媒体查询。
  - `.modal-backdrop`、`.modal`、`.modal h2`、`.modal > header…`、`.modal label/p`、`.modal-actions` 根布局。
  - `.review-backdrop`、`.review-panel`/`.materials-panel`/`.backup-panel` 的宽度/滚动/阴影根规则。
  - `.txt-import`、`.txt-export`、`.book-search`、`.book-replace`、`.chapter-split`、`.tasks-panel`、`.modal.writing-panel` 的根尺寸规则。
  - [style.css](file:///e:/Program%20Files%20(x86)/MoJuanNovelStudio/src/style.css#L1907-L1978) 的重复面板外观块。
- 保留并重新作用域化各面板内部细节（writing/foreshadow/history/material 等）；把 `.review-panel p` 改为 `.review-panel .review-body p` 等明确容器。
- 完成后 grep 确认 `modal-backdrop|review-backdrop` 只在迁移期间出现，最终为 0。

### 3.12 视觉细节规范（本轮重点）

以下为落地时必须遵守的具体值，避免“方向对但做出来还是旧观感”。

**层级与留白**

- 每个页面只保留 3 级视觉层级：页面标题（30px 衬线）→ 区块标题（13–16px/600）→ 正文/元信息（14/12.5/11.5px）。
- 区块之间用 24–32px 间距，区块内部用 12–16px；不对每个元素都加边框，只给卡片/列表容器加 1px `--line`。
- 页面左右留白：书架 `clamp(24px,4vw,56px)`；编辑器正文列 `clamp(20px,4vw,64px)`；弹窗 body 24px、header/footer 24px。
- 同一屏最多一个 `.primary`；其余按钮为默认描边按钮或 `.ghost`。

**颜色使用**

- 墨青 `--accent` 只用于：主按钮、选中态左侧 3px 条、链接/关键数字。
- 金色 `--gold` 只用于：侧栏激活指示、书封装饰线；不大面积铺色。
- 背景层次：`--bg`（App 底）→ `--paper`（写作纸面）→ `--surface`（卡片/弹窗）→ `--surface-muted`（目录/次级区）。
- 错误/危险只用 `--danger` 系；成功用 `--ok` 系；不要用蓝色。

**控件细节**

- 按钮：高 34px（工具栏 30–32、主按钮 38）；圆角 8px；默认白底 + 1px `--control-border` + 轻投影；hover 边框加深、底色 `--control-bg-hover`；active 下压 1px。
- 输入/选择：高 34px；圆角 7px；边框 `--line-strong`；focus 2px 墨青 outline offset 2。
- 列表行：高 32–36px；圆角 7px；hover 浅米底；选中 `--accent-soft` + 左侧 3px 墨青条。
- 图标：工具栏 15px、侧栏 18–19px、关闭 18px；图标颜色比文字低一档。

**弹窗细节**

- 遮罩：`#16282e80` + `blur(3px)`；弹窗进入 160ms `opacity + translateY(6px)`，`prefers-reduced-motion` 下禁用。
- 头部：20px 24px 16px；标题 20px 衬线；副标题 12.5px `--muted`；底部 1px 分隔。
- body：20px 24px 24px；长内容只在这里滚动；滚动条宽 10px、浅灰滑块。
- footer：14px 24px；顶部 1px 分隔；背景 `#fcfaf5`；按钮右对齐，主操作在最右。
- 双栏弹窗（导入/资料/任务/伏笔）：左栏 300px（资料/任务 240px）浅米底、右栏白底；两栏各自独立滚动，不整页滚动。
- 任何弹窗在 900×600 下都不得超出视口：`max-height:calc(100dvh - 32px)`，footer 必须可见。

**编辑器细节**

- 顶部栏与工具栏之间用 1px `--line` 分隔，不再叠加大块阴影。
- 工具栏按钮之间插入 1px×18px 分隔线，间隔 8px；按钮 hover 才出现主色图标。
- 目录栏与正文之间 1px 分隔；目录选中行左侧 3px 墨青条，不整行反白。
- 正文纸面用极轻的左右渐暗（`linear-gradient(90deg,#f4f1e9,#fbf9f3 8%,#fbf9f3 92%,#f4f1e9)`），不加水印/纹理图。
- 章节标题与正文之间留 8px；正文行高 1.95；默认字号 18px。

**书架细节**

- 封面保持 0.72 比例、墨色渐变与左侧书脊阴影；hover 上浮 3px，阴影加深。
- 封面下信息卡与封面之间 10px；信息卡内标题 14px、元信息 12px；操作按钮 28px 高、幽灵样式。
- 书架网格最少 172px 列宽，卡片不被拉得过大；>1440px 时最多 6 列。

**动效**

- 只保留 160–200ms 的 hover/弹窗过渡；不加入滚动视差、悬浮弹跳等装饰动效。
- 所有动效在 `prefers-reduced-motion: reduce` 下关闭。

### 3.13 与原截图对照的验收点

| 原截图问题 | 改造后应看到 |
| --- | --- |
| 弹窗几乎占满窗口，主操作在下方折叠区 | 920px 弹窗居中，头尾固定，主操作始终在 footer 可见 |
| 配置、说明、目录、正文全部纵排 | 左设置 + 右预览双栏，目录与正文各自滚动 |
| 说明文字过长，抢占首屏 | 说明折叠为“分章规则与限制”，默认收起但仍在 DOM |
| 主按钮与次要按钮层级不清 | footer 仅一个主按钮；确认态替换为黄色确认条 + 返回/确认 |
| 背景侧栏底部图标/文字松散 | 侧栏底部分组、状态胶囊、行高统一 |
| 编辑器工具栏像一排散按钮 | 工具栏分组 + 分隔线 + 统一 30px 高度 |
| 正文列在窄窗被压得过窄 | 目录 `clamp` 宽度、正文内边距缩小，900px 窗口下正文列约 510px |

## 4. 文件级改动

### 新增

| 文件 | 内容 |
| --- | --- |
| `src/ui/Dialog.tsx` | Radix Dialog 封装（尺寸、头尾、焦点、关闭门禁） |
| `src/styles/ui.css` | 设计令牌、外壳、书架、编辑器、Dialog、共享卡片/字段、番茄样式 |

### 修改

| 文件 | 改动 |
| --- | --- |
| `package.json` / `yarn.lock` | 增加 `@radix-ui/react-dialog` |
| `src/main.tsx` | 追加 `import "./styles/ui.css";` |
| `src/App.tsx` | 主区域加 `shell-main`；rail 分组/`aria-current`；editor-tools 分组分隔符；表单与确认弹窗改用 `Dialog`；保留全部名称与文案 |
| `src/RecycleBin.tsx` | 永久删除确认改用 `Dialog`，保留 `输入名称确认`、`确认永久删除`、`取消` |
| `src/ImportPanel.tsx` | 双栏重排 + Dialog 骨架；移除手写 focus/Escape；保留业务清理与文案 |
| `src/ProjectImportPanel.tsx` | Dialog 骨架 + 分区卡片；保留字段与预览逻辑 |
| `src/ExportPanel.tsx` | Dialog md；表单/预览卡；`保存TXT` 移到 footer 主操作 |
| `src/SplitPanel.tsx` | Dialog md；两段预览；确认操作 footer |
| `src/ReplacePanel.tsx` | Dialog xl；章节列表 + 结果双栏；主操作 footer |
| `src/SearchPanel.tsx` | Dialog lg；搜索栏 + 结果卡片 |
| `src/AgentAccessPanel.tsx` | Dialog md；状态卡 + 命令块；关闭名称保持 `关闭` |
| `src/BackupPanel.tsx` | Dialog lg；动作导航 + 分区卡片；`dismissible={!pending}` |
| `src/ReviewPanel.tsx` | Dialog xl；tabs 样式统一；`dismissible={!locked}`；保留 `关闭改稿面板` |
| `src/MaterialsPanel.tsx` | Dialog xl + body-flush；左列表/右编辑；`dismissible={!busy}`；保留 `关闭资料库`/`返回全书伏笔` |
| `src/TasksPanel.tsx` | Dialog xl + body-flush；左记录/右报告；删除内联 marginLeft |
| `src/WritingPanel.tsx` | Dialog lg；长表单 body 滚动；`dismissible` 按 busy/needsRead/decision 计算；关闭名称 `关闭写作约束` |
| `src/ForeshadowPanel.tsx` | Dialog xl；左卡列表/右详情；关闭名称 `关闭全书伏笔` |
| `src/FanqieWorkspace.tsx` | 仅补 className 结构（如需），样式在 ui.css 完成 |
| `src/style.css` | 删除旧弹窗/裸 main/重复块，保留并作用域化内部样式 |

### 不改

- `src-tauri/**`、SQLite schema、API 类型、业务状态机。
- 所有按钮可见文案、字段 label、aria 名称、`aria-pressed` 语义。
- 现有测试文件（除非发现纯样式断言需要同步，优先改实现适配测试）。

## 5. 实施顺序

1. **基线**：运行 `yarn test` 记录当前通过/失败；`yarn add @radix-ui/react-dialog`；确认 `yarn build` 可用。
2. **基础层**：新增 `src/styles/ui.css` 与 `src/main.tsx` 导入；先落地令牌、`html/body/#root`、`.app` 网格、`.rail`、`.shell-main`，跑一次 `yarn build`。
3. **主界面**：改 `App.tsx` 外壳/编辑器/书架 class 与工具分组；补 `RecycleBin`、`FanqieWorkspace` 样式；跑 `yarn test -- App` 相关用例。
4. **Dialog 组件**：实现 `src/ui/Dialog.tsx`；先用 `App.tsx` 的两个小弹窗 + `RecycleBin` 验证焦点 / Esc / 关闭名称。
5. **导入弹窗优先重排**：按 3.8 完成左设置 + 右预览双栏与固定 footer，对照原截图检查 1024×678；重点跑 `ImportPanel.test.tsx`、`AppImport.test.tsx`。这是本轮最直观的验收点，先做出来再迁移其他面板。
6. **小面板迁移**：ProjectImport、Export、Split、Replace、Search、AgentAccess；每迁 2-3 个跑对应测试。
7. **大面板迁移**：Backup、Review、Materials、Tasks、Writing、Foreshadow；删除各面板手写 focus/Escape；跑对应测试与全量测试。
8. **CSS 清理**：删除旧弹窗/裸 main/重复块；grep 确认无残留；`yarn test` + `yarn build`。
9. **视觉验收**：`yarn tauri dev`，在 1280×850、1024×678、900×600 三档检查；按 3.12/3.13 的细节清单逐项核对，必要时用 computer-use 截图比对。

## 6. 假设与决策

- 保留墨青 + 米纸国风，不换主题、不做深色模式。
- 只引入 Radix Dialog；不引入完整组件库，避免包体、视觉和测试选择器大面积重写。
- 弹窗点击遮罩不关闭，与现状一致；Esc 与关闭按钮受 `dismissible` 门禁。
- 关闭按钮统一为图标 X，但 `aria-label` 必须逐面板保留原文；可见文案“关闭”不再必需。
- 确认操作继续用 `Dialog`（role=dialog），不用 `AlertDialog`（role=alertdialog）。
- 原生 select/input/textarea 保留，统一用 `.ui-field` 包装。
- 导入说明文本保留在 DOM（可折叠），避免破坏 `getByText` 正则断言。
- 不调整 Tauri 默认窗口 1280×850 与最小 900×600；仅优化响应式。
- 不新增测试文件；以现有 40 个测试文件 + build 作为回归基线。

## 7. 验证

1. **基线**：改动前 `yarn test`；记录既有失败，避免误判。
2. **增量**：每阶段运行相关测试：
   - `ImportPanel.test.tsx`、`AppImport.test.tsx`
   - `MaterialsPanel.test.tsx`、`ReviewPanel.test.tsx`
   - `WritingPanel.test.tsx`、`AppWriting.test.tsx`
   - `BackupPanel.test.tsx`、`ForeshadowPanel.test.tsx`、`ForeshadowProposalPanel.test.tsx`
   - `TasksPanel.test.tsx`、`AppTasks.test.tsx`
   - `App.test.tsx`、`AppExport.test.tsx`、`AppSplit.test.tsx`、`AppReplace.test.tsx`、`AppSearch.test.tsx`
3. **全量**：`yarn test` 全部通过；`yarn build`（`tsc -b && vite build`）通过。
4. **视觉**：`yarn tauri dev` 后按 3.12 的细节值和 3.13 的对照表逐屏核对：
   - 书架：标题/搜索/操作区对齐；封面与信息卡；导入TXT入口；信息卡间距 10px、按钮 28px。
   - 编辑器：顶部栏 56px、工具栏 44px + 分组分隔线；目录选中左条；正文列宽与 1.95 行高；底部状态胶囊。
   - 导入弹窗（对照原截图）：左设置 + 右预览、固定头尾、章节列表与正文预览独立滚动、确认态按钮层级；900×600 下 footer 仍可见。
   - 导出/拆章/替换/搜索/备份/检查任务/创作资料/改稿与历史/写作约束/全书伏笔/智能体连接：头部关闭、主次操作、列表选中、窄窗换行。
   - 番茄章稿：卡片、卷章选择、映射字段、操作按钮。
   - 全局：同一屏仅一个主按钮；无大面积描边按钮噪音；hover/focus/disabled 三态对比度正常。
5. **键盘与可访问性**：Tab 焦点环清晰；Tab 在弹窗内循环；Esc 仅在允许时关闭；关闭后焦点回到触发按钮；disabled 状态不误导。
6. **静态检查**：grep 确认 `main { margin-left` 与 `modal-backdrop/review-backdrop` 无残留；确认无 `style={{ marginLeft: 0 }}` 补丁。

## 8. 风险与回滚

- **Radix Portal/焦点行为差异**：先在小面板验证；若某测试断言 Tab 循环顺序，保留该面板原有的 `initialFocusRef` 或调整 Dialog 的首焦点策略，而不是回退整套组件。
- **测试选择器破坏**：所有 dialog/button/label 名称按第 2.3 节逐项保留；迁移时先跑对应测试再继续。
- **CSS 级联冲突**：`ui.css` 最后加载；旧 `.modal` 后代规则在迁移完成后删除，避免双重命中。
- **依赖安装失败**：停止并报告 registry/网络问题，不静默改用 CSS-only 方案。
- **回滚**：所有改动集中在 `src/ui/`、`src/styles/ui.css`、面板 JSX 的包裹层与 `style.css` 的删除块；可整体回退这些文件，不影响 Rust/SQLite。
