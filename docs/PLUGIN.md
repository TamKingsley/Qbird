# Qbird 预览插件接口（v1，只读）

Qbird 不内置 docx、pptx、pdf、xlsx 等查看器。这些格式由**本地安装的预览插件**在右侧工作区渲染。
第一版只做**只读预览**：插件不能写回附件，不能访问聊天记录、身份密钥、数据库或网络。

## 1. 插件目录

```
<应用数据目录>/plugins/<插件 id>/
  plugin.json
  preview.html
```

用户通过「设置 → 实验功能 → 预览插件 → 从文件夹安装…」把整个目录复制进来；同一 id 已存在时拒绝覆盖。

## 2. plugin.json

```json
{
  "id": "org.example.docx-preview",
  "name": "DOCX 预览",
  "version": "1.0.0",
  "api": 1,
  "preview": [
    { "extensions": ["docx"], "entry": "preview.html" }
  ]
}
```

| 字段 | 规则 |
|---|---|
| `id` | 2–64 个字符，小写字母/数字/`.`/`-`/`_`，以字母或数字开头；全局唯一 |
| `name` | 1–64 个可见字符 |
| `version` | 1–32 个字符，字母、数字、`.`、`-`、`+` |
| `api` | 必须为 `1` |
| `preview` | 1–8 条规则；每条 1–8 个扩展名（小写字母/数字，1–16 字符），扩展名不可重复 |
| `entry` | 相对路径，必须以 `.html`/`.htm` 结尾，不得包含 `..`、绝对路径或空片段；文件须存在且不超过 8 MiB |

插件目录限制：最多 200 个文件、合计 16 MiB、层级不超过 4 层、不允许符号链接。

入口文件应当是**自包含 HTML**（脚本与样式内联）。宿主会注入 CSP，外部脚本、样式、字体与网络请求都会被阻止。

## 3. 宿主与插件的通信

1. 用户在右侧工作区打开一个已完成的附件，扩展名命中插件规则时，宿主创建
   `<iframe sandbox="allow-scripts">`（无同源权限）并加载入口 HTML。
2. iframe 触发 `load` 后，宿主向插件发送一次消息：

```js
{ type: "qbird:preview", name: "季度报告.docx", bytes: ArrayBuffer }
```

`bytes` 是文件原始字节，别无其他数据；文件大于 8 MiB 时不会调用插件。

3. 插件自行渲染内容，可选回执（宿主仅用于诊断，不强制）：

```js
parent.postMessage({ type: "qbird:ready" }, "*");
parent.postMessage({ type: "qbird:error", message: "无法解析该文档" }, "*");
```

最小示例：

```html
<!doctype html>
<html><body>
<div id="out"></div>
<script>
window.addEventListener("message", (event) => {
  const data = event.data;
  if (!data || data.type !== "qbird:preview") return;
  const bytes = new Uint8Array(data.bytes);
  document.getElementById("out").textContent = data.name + " · " + bytes.length + " 字节";
});
</script>
</body></html>
```

## 4. 安全边界

- 隔离：`sandbox="allow-scripts"`，无 `allow-same-origin`，插件处于不透明源，读不到宿主页面、存储或 Tauri API。
- 网络：注入 `default-src 'none'` 的 CSP，禁止 fetch/XHR/外链资源；需要字体的查看器必须内联 base64。
- 数据：只传被预览文件的字节；不传文件名之外的信息（路径、联系人、指纹、群成员均不提供）。
- 故障隔离：插件异常或超时只影响该面板，主界面与其他会话不受影响。
- 安装：仅本地文件夹导入；不支持远程 URL、市场或自动更新；移除需二次确认。
- 只读：第一版不提供写回接口；编辑能力需另立权限模型（写回授权、临时文件、覆盖确认）后再实现。

## 5. 分发与示例

示例插件见 `docs/plugins/qdemo-preview/`：它声明扩展名 `qdemo`，把收到的字节以十六进制片段显示。把该文件夹通过设置页安装后，给任意 `*.qdemo` 文件点“预览”即可验证链路。

不支持的能力（本版）：消息协议扩展、Agent 联系人、原生 DLL 插件、插件市场、插件间通信。
