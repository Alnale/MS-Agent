import { useState, useCallback, useEffect } from 'react';
import type { ChatMessage } from '../api/types';

export interface Session {
  id: string;
  title: string;
  messages: ChatMessage[];
  createdAt: number;
  updatedAt: number;
}

const STORAGE_KEY = 'agent-teams-sessions';

function loadSessions(): Session[] {
  try {
    const data = localStorage.getItem(STORAGE_KEY);
    if (data) {
      return JSON.parse(data);
    }
  } catch (e) {
    console.error('Failed to load sessions:', e);
  }
  return [];
}

function saveSessions(sessions: Session[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(sessions));
  } catch (e) {
    console.error('Failed to save sessions:', e);
  }
}

function generateSessionId(): string {
  return `session-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

function generateTitle(messages: ChatMessage[]): string {
  const firstUserMsg = messages.find((m) => m.role === 'user');
  if (firstUserMsg) {
    const content = firstUserMsg.content;
    return content.length > 30 ? content.slice(0, 30) + '...' : content;
  }
  return 'New Chat';
}

export interface UseSessionReturn {
  sessions: Session[];
  currentSessionId: string | null;
  createSession: () => string;
  loadSession: (sessionId: string) => Session | null;
  saveSession: (sessionId: string, messages: ChatMessage[]) => void;
  deleteSession: (sessionId: string) => void;
  deleteSessions: (sessionIds: string[]) => void;
  setCurrentSessionId: (id: string | null) => void;
}

export function useSession(): UseSessionReturn {
  const [sessions, setSessions] = useState<Session[]>(loadSessions);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);

  // Debounced save to avoid writing to localStorage on every streaming message update
  useEffect(() => {
    const timer = setTimeout(() => {
      saveSessions(sessions);
    }, 500);
    return () => clearTimeout(timer);
  }, [sessions]);

  const createSession = useCallback((): string => {
    const id = generateSessionId();
    const newSession: Session = {
      id,
      title: 'New Chat',
      messages: [],
      createdAt: Date.now(),
      updatedAt: Date.now(),
    };
    setSessions((prev) => [newSession, ...prev]);
    setCurrentSessionId(id);
    return id;
  }, []);

  const loadSession = useCallback(
    (sessionId: string): Session | null => {
      return sessions.find((s) => s.id === sessionId) || null;
    },
    [sessions]
  );

  const saveSession = useCallback(
    (sessionId: string, messages: ChatMessage[]) => {
      setSessions((prev) => {
        const idx = prev.findIndex((s) => s.id === sessionId);
        if (idx >= 0) {
          const updated = [...prev];
          updated[idx] = {
            ...updated[idx],
            messages,
            title: generateTitle(messages),
            updatedAt: Date.now(),
          };
          return updated;
        }
        // Session doesn't exist, create it
        const newSession: Session = {
          id: sessionId,
          title: generateTitle(messages),
          messages,
          createdAt: Date.now(),
          updatedAt: Date.now(),
        };
        return [newSession, ...prev];
      });
    },
    []
  );

  const deleteSession = useCallback(
    (sessionId: string) => {
      setSessions((prev) => prev.filter((s) => s.id !== sessionId));
      if (currentSessionId === sessionId) {
        setCurrentSessionId(null);
      }
    },
    [currentSessionId]
  );

  const deleteSessions = useCallback(
    (sessionIds: string[]) => {
      const idSet = new Set(sessionIds);
      setSessions((prev) => prev.filter((s) => !idSet.has(s.id)));
      if (currentSessionId && idSet.has(currentSessionId)) {
        setCurrentSessionId(null);
      }
    },
    [currentSessionId]
  );

  return {
    sessions,
    currentSessionId,
    createSession,
    loadSession,
    saveSession,
    deleteSession,
    deleteSessions,
    setCurrentSessionId,
  };
}
