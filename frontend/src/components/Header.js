import React from 'react';

export function Header({ connected }) {
  return (
    <header className="header">
      <div className="header-left">
        <h1>
          <span className="logo">🦑</span>
          LimogiAICryptoX
        </h1>
        <span className="subtitle">Triangular Intra-X Arbitrage</span>
      </div>
      
      <div className="header-right">
        <div className={`connection-status ${connected ? 'connected' : 'disconnected'}`}>
          <span className="status-dot"></span>
          <span>{connected ? 'Live' : 'Offline'}</span>
        </div>
      </div>
    </header>
  );
}

export default Header;