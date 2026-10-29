'use client';

import React, { useState, useMemo, useEffect, useCallback } from 'react';
import { ChevronLeft, ChevronRight, Settings, Trash2, Mic, Square, Pencil, NotebookPen, SearchIcon, X, Upload } from 'lucide-react';
import { formatDayLong, formatTime } from '@/lib/format';
import { useRouter, usePathname } from 'next/navigation';
import { useSidebar } from './SidebarProvider';
import type { CurrentMeeting, SidebarItem } from '@/components/Sidebar/SidebarProvider';
import { ConfirmationModal } from '../ConfirmationModel/confirmation-modal';
import { ModelConfig } from '@/components/ModelSettingsModal';
import { SettingTabs } from '../SettingTabs';
import { TranscriptModelProps } from '@/components/TranscriptSettings';
import Analytics from '@/lib/analytics';
import { invoke } from '@tauri-apps/api/core';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { toast } from 'sonner';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useImportDialog } from '@/contexts/ImportDialogContext';
import { useConfig } from '@/contexts/ConfigContext';

import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogTitle,
} from "@/components/ui/dialog"
import { VisuallyHidden } from "@/components/ui/visually-hidden"

import { MessageToast } from '../MessageToast';
import Logo from '../Logo';
import Info from '../Info';
import { ComplianceNotification } from '../ComplianceNotification';
import { Input } from '../ui/input';
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '../ui/input-group';

const Sidebar: React.FC = () => {
  const router = useRouter();
  const pathname = usePathname();
  const {
    currentMeeting,
    setCurrentMeeting,
    sidebarItems,
    isCollapsed,
    toggleCollapse,
    handleRecordingToggle,
    searchTranscripts,
    searchResults,
    isSearching,
    meetings,
    setMeetings,
    serverAddress
  } = useSidebar();

  // Get recording state from RecordingStateContext (single source of truth)
  const { isRecording } = useRecordingState();
  const { openImportDialog } = useImportDialog();
  const { betaFeatures } = useConfig();
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(new Set(['meetings']));
  const [searchQuery, setSearchQuery] = useState<string>('');
  const [showModelSettings, setShowModelSettings] = useState(false);
  const [modelConfig, setModelConfig] = useState<ModelConfig>({
    provider: 'ollama',
    model: '',
    whisperModel: '',
    apiKey: null,
    ollamaEndpoint: null
  });
  const [transcriptModelConfig, setTranscriptModelConfig] = useState<TranscriptModelProps>({
    provider: 'parakeet',
    model: 'parakeet-tdt-0.6b-v3-int8',
  });
  const [settingsSaveSuccess, setSettingsSaveSuccess] = useState<boolean | null>(null);

  // State for edit modal
  const [editModalState, setEditModalState] = useState<{ isOpen: boolean; meetingId: string | null; currentTitle: string }>({
    isOpen: false,
    meetingId: null,
    currentTitle: ''
  });
  const [editingTitle, setEditingTitle] = useState<string>('');

  // Ensure 'meetings' folder is always expanded
  useEffect(() => {
    if (!expandedFolders.has('meetings')) {
      const newExpanded = new Set(expandedFolders);
      newExpanded.add('meetings');
      setExpandedFolders(newExpanded);
    }
  }, [expandedFolders]);

  // useEffect(() => {
  //   if (settingsSaveSuccess !== null) {
  //     const timer = setTimeout(() => {
  //       setSettingsSaveSuccess(null);
  //     }, 3000);
  //   }
  // }, [settingsSaveSuccess]);


  const [deleteModalState, setDeleteModalState] = useState<{ isOpen: boolean; itemId: string | null }>({ isOpen: false, itemId: null });

  useEffect(() => {
    // Note: Don't set hardcoded defaults - let DB be the source of truth
    const fetchModelConfig = async () => {
      // Only make API call if serverAddress is loaded
      if (!serverAddress) {
        console.log('Waiting for server address to load before fetching model config');
        return;
      }

      try {
        const data = await invoke('api_get_model_config') as any;
        if (data && data.provider !== null) {
          // Fetch API key if not included and provider requires it
          if (data.provider !== 'ollama' && !data.apiKey) {
            try {
              const apiKeyData = await invoke('api_get_api_key', {
                provider: data.provider
              }) as string;
              data.apiKey = apiKeyData;
            } catch (err) {
              console.error('Failed to fetch API key:', err);
            }
          }
          setModelConfig(data);
        }
      } catch (error) {
        console.error('Failed to fetch model config:', error);
      }
    };

    fetchModelConfig();
  }, [serverAddress]);


  useEffect(() => {
    // Note: Don't set hardcoded defaults - let DB be the source of truth
    const fetchTranscriptSettings = async () => {
      // Only make API call if serverAddress is loaded
      if (!serverAddress) {
        console.log('Waiting for server address to load before fetching transcript settings');
        return;
      }

      try {
        const data = await invoke('api_get_transcript_config') as any;
        if (data && data.provider !== null) {
          setTranscriptModelConfig(data);
        }
      } catch (error) {
        console.error('Failed to fetch transcript settings:', error);
      }
    };
    fetchTranscriptSettings();
  }, [serverAddress]);

  // Listen for model config updates from other components
  useEffect(() => {
    const setupListener = async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const unlisten = await listen<ModelConfig>('model-config-updated', (event) => {
        console.log('Sidebar received model-config-updated event:', event.payload);
        setModelConfig(event.payload);
      });

      return unlisten;
    };

    let cleanup: (() => void) | undefined;
    setupListener().then(fn => cleanup = fn);

    return () => {
      cleanup?.();
    };
  }, []);



  // Handle model config save
  const handleSaveModelConfig = async (config: ModelConfig) => {
    try {
      await invoke('api_save_model_config', {
        provider: config.provider,
        model: config.model,
        whisperModel: config.whisperModel,
        apiKey: config.apiKey,
        ollamaEndpoint: config.ollamaEndpoint,
      });

      setModelConfig(config);
      console.log('Model config saved successfully');
      setSettingsSaveSuccess(true);

      // Emit event to sync other components
      const { emit } = await import('@tauri-apps/api/event');
      await emit('model-config-updated', config);

      // Track settings change
      await Analytics.trackSettingsChanged('model_config', `${config.provider}_${config.model}`);
    } catch (error) {
      console.error('Error saving model config:', error);
      setSettingsSaveSuccess(false);
    }
  };

  const handleSaveTranscriptConfig = async (updatedConfig?: TranscriptModelProps) => {
    try {
      const configToSave = updatedConfig || transcriptModelConfig;
      const payload = {
        provider: configToSave.provider,
        model: configToSave.model,
        apiKey: configToSave.apiKey ?? null
      };
      console.log('Saving transcript config with payload:', payload);

      await invoke('api_save_transcript_config', {
        provider: payload.provider,
        model: payload.model,
        apiKey: payload.apiKey,
      });


      setSettingsSaveSuccess(true);

      // Track settings change
      const transcriptConfigToSave = updatedConfig || transcriptModelConfig;
      await Analytics.trackSettingsChanged('transcript_config', `${transcriptConfigToSave.provider}_${transcriptConfigToSave.model}`);
    } catch (error) {
      console.error('Failed to save transcript config:', error);
      setSettingsSaveSuccess(false);
    }
  };

  // Handle search input changes
  const handleSearchChange = useCallback(async (value: string) => {
    setSearchQuery(value);

    // If search query is empty, just return to normal view
    if (!value.trim()) return;

    // Search through transcripts
    await searchTranscripts(value);

    // Make sure the meetings folder is expanded when searching
    if (!expandedFolders.has('meetings')) {
      const newExpanded = new Set(expandedFolders);
      newExpanded.add('meetings');
      setExpandedFolders(newExpanded);
    }
  }, [expandedFolders, searchTranscripts]);

  // Combine search results with sidebar items
  const filteredSidebarItems = useMemo(() => {
    if (!searchQuery.trim()) return sidebarItems;

    // If we have search results, highlight matching meetings
    if (searchResults.length > 0) {
      // Get the IDs of meetings that matched in transcripts
      const matchedMeetingIds = new Set(searchResults.map(result => result.id));

      return sidebarItems
        .map(folder => {
          // Always include folders in the results
          if (folder.type === 'folder') {
            if (!folder.children) return folder;

            // Filter children based on search results or title match
            const filteredChildren = folder.children.filter(item => {
              // Include if the meeting ID is in our search results
              if (matchedMeetingIds.has(item.id)) return true;

              // Or if the title matches the search query
              return item.title.toLowerCase().includes(searchQuery.toLowerCase());
            });

            return {
              ...folder,
              children: filteredChildren
            };
          }

          // For non-folder items, check if they match the search
          return (matchedMeetingIds.has(folder.id) ||
            folder.title.toLowerCase().includes(searchQuery.toLowerCase()))
            ? folder : undefined;
        })
        .filter((item): item is SidebarItem => item !== undefined); // Type-safe filter
    } else {
      // Fall back to title-only filtering if no transcript results
      return sidebarItems
        .map(folder => {
          // Always include folders in the results
          if (folder.type === 'folder') {
            if (!folder.children) return folder;

            // Filter children based on search query
            const filteredChildren = folder.children.filter(item =>
              item.title.toLowerCase().includes(searchQuery.toLowerCase())
            );

            return {
              ...folder,
              children: filteredChildren
            };
          }

          // For non-folder items, check if they match the search
          return folder.title.toLowerCase().includes(searchQuery.toLowerCase()) ? folder : undefined;
        })
        .filter((item): item is SidebarItem => item !== undefined); // Type-safe filter
    }
  }, [sidebarItems, searchQuery, searchResults, expandedFolders]);


  const handleDelete = async (itemId: string) => {
    console.log('Deleting item:', itemId);
    const payload = {
      meetingId: itemId
    };

    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('api_delete_meeting', {
        meetingId: itemId,
      });
      console.log('Meeting deleted successfully');
      const updatedMeetings = meetings.filter((m: CurrentMeeting) => m.id !== itemId);
      setMeetings(updatedMeetings);

      // Track meeting deletion
      Analytics.trackMeetingDeleted(itemId);

      // Show success toast
      toast.success("Mødet er slettet", {
        description: "Optagelse, transskript og opsummering er fjernet"
      });

      // If deleting the active meeting, navigate to home
      if (currentMeeting?.id === itemId) {
        setCurrentMeeting({ id: 'intro-call', title: 'Nyt møde' });
        router.push('/');
      }
    } catch (error) {
      console.error('Failed to delete meeting:', error);
      toast.error("Mødet kunne ikke slettes", {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  };

  const handleDeleteConfirm = () => {
    if (deleteModalState.itemId) {
      handleDelete(deleteModalState.itemId);
    }
    setDeleteModalState({ isOpen: false, itemId: null });
  };

  // Handle modal editing of meeting names
  const handleEditStart = (meetingId: string, currentTitle: string) => {
    setEditModalState({
      isOpen: true,
      meetingId: meetingId,
      currentTitle: currentTitle
    });
    setEditingTitle(currentTitle);
  };

  const handleEditConfirm = async () => {
    const newTitle = editingTitle.trim();
    const meetingId = editModalState.meetingId;

    if (!meetingId) return;

    // Prevent empty titles
    if (!newTitle) {
      toast.error("Mødet skal have et navn");
      return;
    }

    try {
      await invoke('api_save_meeting_title', {
        meetingId: meetingId,
        title: newTitle,
      });

      // Update local state
      const updatedMeetings = meetings.map((m: CurrentMeeting) =>
        m.id === meetingId ? { ...m, title: newTitle } : m
      );
      setMeetings(updatedMeetings);

      // Update current meeting if it's the one being edited
      if (currentMeeting?.id === meetingId) {
        setCurrentMeeting({ id: meetingId, title: newTitle });
      }

      // Track the edit
      Analytics.trackButtonClick('edit_meeting_title', 'sidebar');

      toast.success("Mødet er omdøbt");

      // Close modal and reset state
      setEditModalState({ isOpen: false, meetingId: null, currentTitle: '' });
      setEditingTitle('');
    } catch (error) {
      console.error('Failed to update meeting title:', error);
      toast.error("Mødet kunne ikke omdøbes", {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  };

  const handleEditCancel = () => {
    setEditModalState({ isOpen: false, meetingId: null, currentTitle: '' });
    setEditingTitle('');
  };

  const toggleFolder = (folderId: string) => {
    // Normal toggle behavior for all folders
    const newExpanded = new Set(expandedFolders);
    if (newExpanded.has(folderId)) {
      newExpanded.delete(folderId);
    } else {
      newExpanded.add(folderId);
    }
    setExpandedFolders(newExpanded);
  };

  // Expose setShowModelSettings to window for Rust tray to call
  useEffect(() => {
    (window as any).openSettings = () => {
      setShowModelSettings(true);
    };

    // Cleanup on unmount
    return () => {
      delete (window as any).openSettings;
    };
  }, []);

  // Find matching transcript snippet for a meeting item
  const findMatchingSnippet = (itemId: string) => {
    if (!searchQuery.trim() || !searchResults.length) return null;
    return searchResults.find(result => result.id === itemId);
  };

  const isMeetingId = (id: string) => id.includes('-') && !id.startsWith('intro-call');

  // Mødelisten grupperes pr. dag: "I dag", "I går", ellers "torsdag 22. oktober"
  const meetingGroups = useMemo(() => {
    const folder = filteredSidebarItems.find(item => item.type === 'folder' && item.id === 'meetings');
    const children = (folder?.children ?? []).filter(child => isMeetingId(child.id));
    const today = new Date();
    const dayKey = (d: Date) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
    const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
    const groups: { key: string; label: string; items: SidebarItem[] }[] = [];
    for (const child of children) {
      const created = child.createdAt ? new Date(child.createdAt) : null;
      const valid = created && !Number.isNaN(created.getTime());
      const key = valid ? dayKey(created) : 'ukendt';
      let group = groups.find(g => g.key === key);
      if (!group) {
        const label = !valid ? 'Tidligere'
          : key === dayKey(today) ? 'I dag'
          : key === dayKey(yesterday) ? 'I går'
          : formatDayLong(created);
        group = { key, label, items: [] };
        groups.push(group);
      }
      group.items.push(child);
    }
    return groups;
  }, [filteredSidebarItems]);

  const openMeeting = (item: SidebarItem) => {
    setCurrentMeeting({ id: item.id, title: item.title });
    router.push(`/meeting-details?id=${item.id}`);
  };

  const iconButton = 'inline-flex items-center justify-center h-8 w-8 rounded-md text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring';

  const renderMeeting = (item: SidebarItem) => {
    const isActive = currentMeeting?.id === item.id;
    const matchingResult = findMatchingSnippet(item.id);

    return (
      <li key={item.id} className="group relative">
        <button
          type="button"
          onClick={() => openMeeting(item)}
          aria-current={isActive ? 'page' : undefined}
          className={`w-full text-left rounded-md px-3 py-2 min-h-[44px] transition-colors focus-visible:ring-2 focus-visible:ring-ring ${
            isActive ? 'bg-accent text-foreground' : 'hover:bg-accent/60 text-foreground'
          }`}
        >
          <span className={`block text-sm line-clamp-2 break-words ${isActive ? 'font-semibold' : 'font-medium'}`}>{item.title}</span>
          {item.createdAt && (
            <span className="block text-xs text-muted-foreground tabular-nums">{formatTime(item.createdAt)}</span>
          )}
          {matchingResult && (
            <span className="mt-1 block text-xs text-muted-foreground line-clamp-2">
              {matchingResult.matchContext}
            </span>
          )}
        </button>
        <div className="absolute right-1 top-1.5 flex gap-0.5 rounded-md bg-accent opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 transition-opacity">
          <button
            type="button"
            onClick={() => handleEditStart(item.id, item.title)}
            className={iconButton}
            aria-label={`Omdøb mødet ${item.title}`}
            title="Omdøb"
          >
            <Pencil className="w-4 h-4" aria-hidden="true" />
          </button>
          <button
            type="button"
            onClick={() => setDeleteModalState({ isOpen: true, itemId: item.id })}
            className={`${iconButton} hover:text-destructive`}
            aria-label={`Slet mødet ${item.title}`}
            title="Slet"
          >
            <Trash2 className="w-4 h-4" aria-hidden="true" />
          </button>
        </div>
      </li>
    );
  };

  const recordButtonLabel = isRecording ? 'Optagelse i gang' : 'Ny optagelse';

  const renderCollapsedIcons = () => {
    const isSettingsPage = pathname === '/settings';
    return (
      <TooltipProvider>
        <div className="flex flex-col items-center gap-3 pt-4">
          <Logo isCollapsed />
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={handleRecordingToggle}
                disabled={isRecording}
                aria-label={recordButtonLabel}
                className="h-11 w-11 inline-flex items-center justify-center rounded-full bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-100 disabled:bg-recording/15 disabled:text-recording"
              >
                {isRecording ? <Square className="w-5 h-5" aria-hidden="true" /> : <Mic className="w-5 h-5" aria-hidden="true" />}
              </button>
            </TooltipTrigger>
            <TooltipContent side="right"><p>{recordButtonLabel}</p></TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={toggleCollapse}
                aria-label="Vis mødelisten"
                className="h-11 w-11 inline-flex items-center justify-center rounded-lg hover:bg-accent"
              >
                <NotebookPen className="w-5 h-5 text-muted-foreground" aria-hidden="true" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right"><p>Møder</p></TooltipContent>
          </Tooltip>
          {betaFeatures.importAndRetranscribe && (
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  onClick={() => openImportDialog()}
                  aria-label="Importér lyd"
                  className="h-11 w-11 inline-flex items-center justify-center rounded-lg hover:bg-accent"
                >
                  <Upload className="w-5 h-5 text-muted-foreground" aria-hidden="true" />
                </button>
              </TooltipTrigger>
              <TooltipContent side="right"><p>Importér lyd</p></TooltipContent>
            </Tooltip>
          )}
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={() => router.push('/settings')}
                aria-label="Indstillinger"
                className={`h-11 w-11 inline-flex items-center justify-center rounded-lg ${isSettingsPage ? 'bg-accent' : 'hover:bg-accent'}`}
              >
                <Settings className="w-5 h-5 text-muted-foreground" aria-hidden="true" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right"><p>Indstillinger</p></TooltipContent>
          </Tooltip>
          <Info isCollapsed />
        </div>
      </TooltipProvider>
    );
  };

  return (
    <div className="fixed top-0 left-0 h-screen z-40">
      <nav
        aria-label="Hovedmenu"
        className={`relative h-screen bg-card border-r border-border flex flex-col transition-[width] duration-200 ${isCollapsed ? 'w-16' : 'w-64'}`}
      >
        <button
          type="button"
          onClick={toggleCollapse}
          aria-label={isCollapsed ? 'Fold sidepanelet ud' : 'Fold sidepanelet sammen'}
          aria-expanded={!isCollapsed}
          className="absolute -right-3 top-5 z-50 h-6 w-6 inline-flex items-center justify-center rounded-full bg-card border border-border text-muted-foreground hover:text-foreground hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
        >
          {isCollapsed ? <ChevronRight className="w-4 h-4" aria-hidden="true" /> : <ChevronLeft className="w-4 h-4" aria-hidden="true" />}
        </button>

        {isCollapsed ? (
          renderCollapsedIcons()
        ) : (
          <>
            <div className="flex-shrink-0 px-4 pt-5 pb-3">
              <Logo isCollapsed={false} />
              <button
                type="button"
                onClick={handleRecordingToggle}
                disabled={isRecording}
                className={`mt-4 w-full inline-flex items-center justify-center gap-2 min-h-[44px] px-3 rounded-lg text-sm font-semibold transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-card ${
                  isRecording
                    ? 'bg-recording/10 text-recording cursor-default'
                    : 'bg-primary text-primary-foreground hover:bg-primary/90'
                }`}
              >
                {isRecording ? (
                  <>
                    <span className="h-2.5 w-2.5 rounded-full bg-recording motion-safe:animate-pulse" aria-hidden="true" />
                    <span>Optagelse i gang</span>
                  </>
                ) : (
                  <>
                    <Mic className="w-4 h-4" aria-hidden="true" />
                    <span>Ny optagelse</span>
                  </>
                )}
              </button>

              <div className="mt-3">
                <InputGroup>
                  <InputGroupInput
                    placeholder="Søg i møder"
                    aria-label="Søg i møder og transskripter"
                    value={searchQuery}
                    onChange={(e) => handleSearchChange(e.target.value)}
                  />
                  <InputGroupAddon>
                    <SearchIcon aria-hidden="true" />
                  </InputGroupAddon>
                  {searchQuery && (
                    <InputGroupAddon align="inline-end">
                      <InputGroupButton onClick={() => handleSearchChange('')} aria-label="Ryd søgning">
                        <X />
                      </InputGroupButton>
                    </InputGroupAddon>
                  )}
                </InputGroup>
              </div>
            </div>

            <div className="flex-1 min-h-0 overflow-y-auto custom-scrollbar px-2 pb-2" aria-live="polite">
              {searchQuery && isSearching && (
                <p className="px-3 py-2 text-sm text-muted-foreground">Søger…</p>
              )}
              {meetingGroups.length === 0 ? (
                <div className="px-3 py-6 text-sm text-muted-foreground">
                  {searchQuery ? (
                    <p>Ingen møder matcher "{searchQuery}". Prøv et andet ord.</p>
                  ) : (
                    <>
                      <p className="font-medium text-foreground">Ingen møder endnu</p>
                      <p className="mt-1">Tryk på "Ny optagelse", så dukker dit første møde op her.</p>
                    </>
                  )}
                </div>
              ) : (
                meetingGroups.map(group => (
                  <section key={group.key} className="mt-3 first:mt-0" aria-label={group.label}>
                    <h2 className="px-3 pb-1 text-xs font-medium text-muted-foreground font-sans tracking-normal">{group.label}</h2>
                    <ul className="space-y-0.5">{group.items.map(renderMeeting)}</ul>
                  </section>
                ))
              )}
            </div>

            <div className="flex-shrink-0 border-t border-border p-2 flex flex-col gap-0.5">
              {betaFeatures.importAndRetranscribe && (
                <button
                  type="button"
                  onClick={() => openImportDialog()}
                  className="flex items-center gap-2 min-h-[44px] px-3 rounded-lg text-sm text-foreground hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
                >
                  <Upload className="w-4 h-4" aria-hidden="true" />
                  Importér lyd
                </button>
              )}
              <div className="flex items-center gap-1">
              <button
                type="button"
                onClick={() => router.push('/settings')}
                aria-current={pathname === '/settings' ? 'page' : undefined}
                className={`flex-1 inline-flex items-center gap-2 min-h-[44px] px-3 rounded-lg text-sm text-foreground hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring ${pathname === '/settings' ? 'bg-accent' : ''}`}
              >
                <Settings className="w-4 h-4" aria-hidden="true" />
                Indstillinger
              </button>
              <Info isCollapsed />
              </div>
            </div>
          </>
        )}
      </nav>

      <ConfirmationModal
        isOpen={deleteModalState.isOpen}
        text="Vil du slette dette møde? Optagelse, transskript og opsummering fjernes, og det kan ikke fortrydes."
        onConfirm={handleDeleteConfirm}
        onCancel={() => setDeleteModalState({ isOpen: false, itemId: null })}
      />

      <Dialog open={editModalState.isOpen} onOpenChange={(open) => {
        if (!open) handleEditCancel();
      }}>
        <DialogContent className="sm:max-w-[425px]">
          <VisuallyHidden>
            <DialogTitle>Omdøb møde</DialogTitle>
          </VisuallyHidden>
          <div className="py-4">
            <h3 className="text-lg font-semibold mb-4">Omdøb møde</h3>
            <label htmlFor="meeting-title" className="block text-sm font-medium text-foreground mb-2">
              Mødets navn
            </label>
            <input
              id="meeting-title"
              type="text"
              value={editingTitle}
              onChange={(e) => setEditingTitle(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  handleEditConfirm();
                } else if (e.key === 'Escape') {
                  handleEditCancel();
                }
              }}
              className="w-full min-h-[44px] px-3 border border-input bg-background rounded-md focus-visible:ring-2 focus-visible:ring-ring"
              placeholder="Skriv et navn"
              autoFocus
            />
          </div>
          <DialogFooter>
            <button
              type="button"
              onClick={handleEditCancel}
              className="min-h-[44px] px-4 text-sm font-medium text-foreground bg-secondary hover:bg-accent rounded-md transition-colors"
            >
              Annullér
            </button>
            <button
              type="button"
              onClick={handleEditConfirm}
              className="min-h-[44px] px-4 text-sm font-medium text-primary-foreground bg-primary hover:bg-primary/90 rounded-md transition-colors"
            >
              Gem
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
};

export default Sidebar;
