import React from "react";
import { Dialog, DialogContent, DialogTitle, DialogTrigger } from "./ui/dialog";
import { VisuallyHidden } from "./ui/visually-hidden";
import { About } from "./About";

/** Fjerpen med skrivelinje. Tegnes med currentColor, så den følger lys/mørk tilstand. */
export function TingskriverMark({ className = "h-7 w-7" }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 32 32"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M6.5 25.5C6 14 14 6 26 6c0 12-8 19.5-19.5 19.500Z" />
      <path d="M4 28 18 14" />
      <path d="M19 28h9" />
    </svg>
  );
}

interface LogoProps {
  isCollapsed: boolean;
}

const Logo = React.forwardRef<HTMLButtonElement, LogoProps>(
  ({ isCollapsed }, ref) => {
    return (
      <Dialog aria-describedby={undefined}>
        <DialogTrigger asChild>
          <button
            ref={ref}
            type="button"
            className={`flex items-center gap-2 text-primary rounded-md focus-visible:ring-2 focus-visible:ring-ring ${
              isCollapsed ? "h-11 w-11 justify-center" : "min-h-[44px] px-1"
            }`}
            aria-label="Om Tingskriver"
          >
            <TingskriverMark className={isCollapsed ? "h-7 w-7" : "h-6 w-6"} />
            {!isCollapsed && (
              <span className="font-serif text-xl font-semibold tracking-tight text-foreground">Tingskriver</span>
            )}
          </button>
        </DialogTrigger>
        <DialogContent>
          <VisuallyHidden>
            <DialogTitle>Om Tingskriver</DialogTitle>
          </VisuallyHidden>
          <About />
        </DialogContent>
      </Dialog>
    );
  },
);

Logo.displayName = "Logo";

export default Logo;
