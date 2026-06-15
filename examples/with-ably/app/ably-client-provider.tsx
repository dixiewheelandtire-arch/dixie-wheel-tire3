"use client";

import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import * as Ably from "ably";
import { AblyProvider, ChannelProvider } from "ably/react";

const AblyReadyContext = createContext(false);

export function useAblyReady() {
  return useContext(AblyReadyContext);
}

function randomClientId() {
  return (
    Math.random().toString(36).substring(2, 15) +
    Math.random().toString(36).substring(2, 15)
  );
}

export default function AblyClientProvider({
  children,
}: {
  children: ReactNode;
}) {
  const [clientId] = useState(() => randomClientId());
  const [client, setClient] = useState<Ably.Realtime | null>(null);

  useEffect(() => {
    const ably = new Ably.Realtime({
      authUrl: `/api/createTokenRequest?clientId=${clientId}`,
      clientId,
    });
    setClient(ably);
    return () => {
      ably.close();
    };
  }, [clientId]);

  if (!client) {
    return (
      <AblyReadyContext.Provider value={false}>
        {children}
      </AblyReadyContext.Provider>
    );
  }

  return (
    <AblyProvider client={client}>
      <ChannelProvider channelName="some-channel-name">
        <AblyReadyContext.Provider value={true}>
          {children}
        </AblyReadyContext.Provider>
      </ChannelProvider>
    </AblyProvider>
  );
}
