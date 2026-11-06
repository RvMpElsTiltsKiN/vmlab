import type { ReactNode } from 'react';

function Svg({ children, size = 16 }: { children: ReactNode; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {children}
    </svg>
  );
}

export const Logo = ({ size = 20 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
    <rect width="32" height="32" rx="8" fill="var(--accent)" />
    <path d="M12 6h8M14 6v7l-5 9a2 2 0 0 0 1.8 3h10.4a2 2 0 0 0 1.8-3l-5-9V6" fill="none" stroke="var(--on-accent)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
    <circle cx="16" cy="21" r="1.6" fill="var(--on-accent)" />
  </svg>
);

export const CubeIcon = () => <Svg><path d="M21 8 12 3 3 8v8l9 5 9-5V8Z" /><path d="m3 8 9 5 9-5M12 13v8" /></Svg>;
export const ContainerIcon = () => <Svg><rect x="3" y="7" width="18" height="11" rx="2" /><path d="M8 7v11M12 7v11M16 7v11" /></Svg>;
export const PlayIcon = () => <Svg size={14}><path d="m7 4 13 8-13 8V4Z" fill="currentColor" /></Svg>;
export const StopIcon = () => <Svg size={14}><rect x="5" y="5" width="14" height="14" rx="2" fill="currentColor" /></Svg>;
export const RestartIcon = () => <Svg size={14}><path d="M21 12a9 9 0 1 1-3-6.7L21 8M21 3v5h-5" /></Svg>;
export const RefreshIcon = () => <Svg size={14}><path d="M21 12a9 9 0 1 1-3-6.7L21 8M21 3v5h-5" /></Svg>;
export const SunIcon = () => <Svg><circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" /></Svg>;
export const MoonIcon = () => <Svg><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8Z" /></Svg>;
export const NetworkIcon = () => <Svg size={14}><circle cx="5" cy="12" r="2.5" /><circle cx="19" cy="6" r="2.5" /><circle cx="19" cy="18" r="2.5" /><path d="m7.2 11 9.6-4M7.2 13l9.6 4" /></Svg>;
export const TrashIcon = () => <Svg size={14}><path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3" /></Svg>;
export const PlusIcon = () => <Svg size={14}><path d="M12 5v14M5 12h14" /></Svg>;
export const LockIcon = () => <Svg size={14}><rect x="3" y="11" width="18" height="10" rx="2" /><path d="M7 11V7a5 5 0 0 1 10 0v4" /></Svg>;
export const UnlockIcon = () => <Svg size={14}><rect x="3" y="11" width="18" height="10" rx="2" /><path d="M7 11V7a5 5 0 0 1 9.9-1" /></Svg>;
export const CheckIcon = () => <Svg size={12}><path d="m5 12.5 4.5 4.5L19 6.5" /></Svg>;
export const WarnIcon = () => <Svg size={12}><path d="M12 3 2.5 20h19L12 3Z" /><path d="M12 9v4.5M12 17h.01" /></Svg>;

export const MinIcon = () => <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 5h10" stroke="currentColor" /></svg>;
export const MaxIcon = () => <svg width="10" height="10" viewBox="0 0 10 10"><rect x=".5" y=".5" width="9" height="9" fill="none" stroke="currentColor" /></svg>;
export const RestoreIcon = () => <svg width="10" height="10" viewBox="0 0 10 10"><path d="M2.5 2.5v-2h7v7h-2M.5 2.5h7v7h-7z" fill="none" stroke="currentColor" /></svg>;
export const CloseIcon = () => <svg width="10" height="10" viewBox="0 0 10 10"><path d="m0 0 10 10M10 0 0 10" stroke="currentColor" /></svg>;
