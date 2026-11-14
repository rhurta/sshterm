export interface Connection {
  id: string;
  name: string;
  host: string;
  port: number;
  user: string;
  password?: string | null;
  keepalive_secs: number;
}

/** What `conn_list` returns: never contains the password. */
export interface ConnectionSummary {
  id: string;
  name: string;
  host: string;
  port: number;
  user: string;
}

export interface Tab {
  sessionId: string;
  name: string;
}
