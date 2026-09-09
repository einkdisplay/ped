# PED, Potato E-Ink Display

土豆墨水屏显示器（PED）是一个可以让你轻易地将 Kindle 转变为自定义屏幕的工具。

## 介绍

PED 是一个网页浏览器。它只能打开一个标签页，并能通过 CPU 渲染网页，最后将网页内容使用 [NiLuJe 的 FBInk](https://github.com/NiLuJe/FBInk) 直接刷新到墨水屏上，而不经过 Kindle 的 X11 显示服务器。

PED 也是一个实验。它使用 Rust + Servo 构建，旨在探索 Servo 这一新兴的浏览器内核在嵌入式设备上（即 Kindle）长期运行时的表现。

### 我为什么不直接用体验版网页浏览器？

这是因为，和 PED 比起来，Kindle 内置的体验版网页浏览器有若干问题。

#### 关不掉的顶栏

Kindle 内置网页浏览器有一个关不掉的顶栏，并且会占据整个屏幕大约 1/6 的空间。虽然不是大问题但作为信息台实在有碍美观。

PED 完全绕过了 Kindle OS，将网页直接刷到墨水屏上，所以可以完美地全屏显示。

#### 古老

在我的 PW3（固件版本 5.16.2.1.1）上，Kindle 内置浏览器的内核是 WebKit 531.2。该版本系统固件的发布日期是 2019 年，然而，根据社区研究，该版本 WebKit 内核的发布日期是 2009 年。它甚至连 Flexbox 都不支持。除非你真的很喜欢给你的网页打 Polyfills，不然你不会想要用内置浏览器的。

相比之下，Servo 虽然仍处于实验性阶段，还未步出“早期开发”，但目前为止它已经支持大部分现代网页使用的特性，应该足以让你用现代技术舒服地构建自己的信息台。

#### 限制

Kindle 内置浏览器会通过某种手段检测网页上的动态元素，并且会弹窗拒绝观看该网页。PED 就没有这个限制。

#### 难以精细控制屏幕刷新

PED 提供了 `window.navigator.kindle` JS API， *([查看详情](./js-binding))* 可以让你能够用代码精细地控制屏幕刷新的时机、方法、区域等。

然而，Kindle 内置浏览器就没有这个能力，完全听天由命。

## 安装

- **前置条件**：一台越狱了的 Kindle。

Coming soon!

## 使用

安装后，在 KUAL 菜单中或你的图书馆中打开 PED，应该就可以了。

在使用 PED 之前，我们推荐你安装 [USBNetwork](https://wiki.mobileread.com/wiki/USBNetwork) 插件，并正确 set up 从你的电脑到 Kindle 的 SSH 连接。充分测试之后，再启动 PED。

## 配置

PED 使用 TOML 格式的配置文件。在打开 PED 之前，请务必先在电脑上编辑好配置文件并放到指定位置。

见 [TOML 文件中的注释](./ped.toml)。

## 构建

在开始之前，你需要：

- Rust 编译工具链
- Podman 或 Docker
- [Cross](https://github.com/cross-rs/cross)

见 [DEVELOPMENT.md](./DEVELOPMENT.md)。

## 为 PED 编写网页

### TypeScript 类型

[![NPM Version](https://img.shields.io/npm/v/potatoeinkdisplay-types)](https://npmjs.com/package/potatoeinkdisplay-types)

你可以用 PED 的浏览器 Kindle API（`window.navigator.kindle`）来读取设备信息（网络、屏幕和电池）以及控制屏幕刷新。我们将相应的 TypeScript
类型定义发布到了 NPM，包名是 `potatoeinkdisplay-types`。

你可以把它安装成 `devDependencies`：

```bash
pnpm add -D potatoeinkdisplay-types
```

然后在你的入口文件中添加：

```typescript
/// <reference types="potatoeinkdisplay-types" />
```

来让 TypeScript 编译器认识 PED 的类型。

### 测试

PED 使用 Servo 浏览器内核，它有可能不支持你在开发过程中使用的部分 Web 技术。

因此，在呈现最终代码之前，请务必下载 [Servo Shell](https://github.com/servo/servo/releases/tag/v0.5.0) 来测试你的代码在 Servo 上的呈现效果。

> PED 基于 Servo 0.6.0 的某个开发版本。使用上述基于 Servo 0.5.0 的 Servo Shell 通常已经足够，但如果你想要 1:1 parity，你可以自行从[上游的该版本 Servo](https://github.com/servo/servo/commit/55964f7d6d50872b51d8f94ef03ad10ac0bbcf1e)（Commit SHA1: `55964f7`）构建 Servo Shell。

## 开源协议

MIT

> 本仓库内有[来自上游 FBInk 的代码](./crates/fbink-sys/)，那一部分使用 GPLv3 开源并授权。
