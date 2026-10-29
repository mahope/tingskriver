'use client';

import { motion } from 'framer-motion';
import { FileQuestion, Sparkles } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';

interface EmptyStateSummaryProps {
  onGenerate: () => void;
  hasModel: boolean;
  isGenerating?: boolean;
  error?: string | null;
}

export function EmptyStateSummary({
  onGenerate,
  hasModel,
  isGenerating = false,
  error = null,
}: EmptyStateSummaryProps) {
  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
      className="flex flex-col items-center justify-center h-full p-8 text-center"
    >
      <FileQuestion className="w-16 h-16 text-muted-foreground mb-4" />
      <h3 className="text-lg font-semibold text-foreground mb-2">
        Ingen opsummering endnu
      </h3>
      <p className="text-sm text-muted-foreground mb-6 max-w-md">
        Lad AI lave en opsummering af mødets transskript med hovedpunkter, handlepunkter og beslutninger.
      </p>

      {error && (
        <p role="alert" className="mb-4 max-w-md rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {error}
        </p>
      )}

      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger asChild>
            <div>
              <Button
                onClick={onGenerate}
                className="gap-2 min-h-[44px]"
                disabled={!hasModel || isGenerating}
              >
                <Sparkles className="w-4 h-4" />
                {isGenerating ? 'Opsummerer…' : error ? 'Prøv igen' : 'Lav opsummering'}
              </Button>
            </div>
          </TooltipTrigger>
          {!hasModel && (
            <TooltipContent>
              <p>Vælg en model under Indstillinger først</p>
            </TooltipContent>
          )}
        </Tooltip>
      </TooltipProvider>

      {!hasModel && (
        <p className="text-xs text-warning mt-3">
          Vælg en model under Indstillinger først
        </p>
      )}
    </motion.div>
  );
}
