import { useState, useEffect, useCallback, useRef } from 'react';

const WS_URL = process.env.REACT_APP_WS_URL || 'ws://localhost:8000';

export function useWebSocket() {
  const [connected, setConnected] = useState(false);
  const [lastMessage, setLastMessage] = useState(null);
  const wsRef = useRef(null);
  const reconnectTimeoutRef = useRef(null);
  const reconnectAttempts = useRef(0);

  const connect = useCallback(() => {
    try {
      const ws = new WebSocket(`${WS_URL}/ws`);

      ws.onopen = () => {
        console.log('WebSocket connected');
        setConnected(true);
        reconnectAttempts.current = 0;

        // Subscribe to channels
        ws.send(JSON.stringify({ type: 'subscribe', channel: 'opportunities' }));
        ws.send(JSON.stringify({ type: 'subscribe', channel: 'prices' }));
      };

      ws.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data);
          setLastMessage(data);
        } catch (err) {
          console.error('Failed to parse WebSocket message:', err);
        }
      };

      ws.onclose = () => {
        console.log('WebSocket disconnected');
        setConnected(false);
        wsRef.current = null;

        // Reconnect with exponential backoff
        const delay = Math.min(1000 * Math.pow(2, reconnectAttempts.current), 30000);
        reconnectAttempts.current += 1;

        reconnectTimeoutRef.current = setTimeout(() => {
          console.log(`Attempting to reconnect (attempt ${reconnectAttempts.current})...`);
          connect();
        }, delay);
      };

      ws.onerror = (error) => {
        console.error('WebSocket error:', error);
      };

      wsRef.current = ws;
    } catch (err) {
      console.error('Failed to create WebSocket:', err);
    }
  }, []);

  const disconnect = useCallback(() => {
    if (reconnectTimeoutRef.current) {
      clearTimeout(reconnectTimeoutRef.current);
    }
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
  }, []);

  const send = useCallback((message) => {
    if (wsRef.current && wsRef.current.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify(message));
    }
  }, []);

  useEffect(() => {
    connect();
    return () => disconnect();
  }, [connect, disconnect]);

  return {
    connected,
    lastMessage,
    send,
    reconnect: connect,
  };
}

export default useWebSocket;
