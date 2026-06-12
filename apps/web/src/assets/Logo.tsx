import logoUrl from "./logo.svg";

/**
 * Herald MCServerMCP Logo —— 静态 SVG 版本（推荐）。
 *
 * 设计意图见 `logo.svg` 头部注释：等距 MC 立方体 + 心跳波纹 + 印鉴断口环。
 * 颜色与 Herald 桌面端共用同一套紫蓝渐变（c4b5fd → a78bfa → 38bdf8），
 * 保持品牌血统一致但形制独立。
 */
export function Logo({
  size = 28,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <img
      src={logoUrl}
      width={size}
      height={size}
      alt="Herald MCServerMCP"
      className={`select-none drop-shadow-[0_0_12px_rgba(167,139,250,0.35)] ${className}`}
      draggable={false}
    />
  );
}

/**
 * 内联版（避免外部 SVG 请求 / 提供独立 ID 命名空间），用于 favicon 之外的
 * 嵌入场景（例如登录卡里需要参与渐变层级控制时）。
 */
export function LogoInline({
  size = 28,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  const id = `mcs-${Math.random().toString(36).slice(2, 7)}`;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      fill="none"
      className={className}
      xmlns="http://www.w3.org/2000/svg"
    >
      <defs>
        <linearGradient id={`${id}-g`} x1="6" y1="6" x2="58" y2="58" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#c4b5fd" />
          <stop offset="0.55" stopColor="#a78bfa" />
          <stop offset="1" stopColor="#38bdf8" />
        </linearGradient>
        <linearGradient id={`${id}-top`} x1="20" y1="18" x2="44" y2="30" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#c4b5fd" />
          <stop offset="1" stopColor="#38bdf8" />
        </linearGradient>
        <radialGradient id={`${id}-r`} cx="50%" cy="50%" r="50%">
          <stop offset="0.65" stopColor="transparent" />
          <stop offset="1" stopColor="#a78bfa" stopOpacity="0.18" />
        </radialGradient>
      </defs>

      <circle cx="32" cy="32" r="28" fill={`url(#${id}-r)`} />
      <path
        d="M 32 4 A 28 28 0 0 1 60 32 M 60 32 A 28 28 0 0 1 32 60 M 32 60 A 28 28 0 0 1 4 32 M 4 32 A 28 28 0 0 1 32 4"
        stroke={`url(#${id}-g)`}
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeDasharray="36 6"
        fill="none"
      />
      <circle cx="32" cy="32" r="22.5" fill="none" stroke={`url(#${id}-g)`} strokeWidth="0.5" strokeOpacity="0.4" />

      {/* 心跳波纹 */}
      <path
        d="M 14 32 A 18 18 0 0 1 32 14"
        stroke={`url(#${id}-g)`}
        strokeWidth="1.2"
        strokeLinecap="round"
        strokeOpacity="0.7"
        fill="none"
      />
      <path
        d="M 50 32 A 18 18 0 0 1 32 50"
        stroke={`url(#${id}-g)`}
        strokeWidth="1.2"
        strokeLinecap="round"
        strokeOpacity="0.7"
        fill="none"
      />

      {/* 立方体三面 */}
      <path
        d="M 32 21 L 43 27 L 32 33 L 21 27 Z"
        fill={`url(#${id}-top)`}
        fillOpacity="0.95"
        stroke={`url(#${id}-g)`}
        strokeWidth="1.4"
        strokeLinejoin="round"
      />
      <path
        d="M 21 27 L 32 33 L 32 45 L 21 39 Z"
        fill="#a78bfa"
        fillOpacity="0.16"
        stroke={`url(#${id}-g)`}
        strokeWidth="1.4"
        strokeLinejoin="round"
      />
      <path
        d="M 43 27 L 32 33 L 32 45 L 43 39 Z"
        fill="#38bdf8"
        fillOpacity="0.10"
        stroke={`url(#${id}-g)`}
        strokeWidth="1.4"
        strokeLinejoin="round"
      />

      <circle cx="32" cy="27" r="1.6" fill="#0a0a0c" />
      <circle cx="32" cy="27" r="0.9" fill="#c4b5fd" />
    </svg>
  );
}
