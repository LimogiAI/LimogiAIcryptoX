"""
API Key Authentication for LimogiAICryptoX

Provides simple but secure API key authentication for protecting
trading endpoints from unauthorized access.

Usage:
    1. Set LIMOGI_API_KEY in your .env file
    2. Add `dependencies=[Depends(require_api_key)]` to protected routes
    3. Frontend sends key via X-API-Key header or ?api_key query param
"""
import secrets
import hashlib
from typing import Optional
from fastapi import HTTPException, Security, Depends, Request
from fastapi.security import APIKeyHeader, APIKeyQuery
from starlette.status import HTTP_401_UNAUTHORIZED, HTTP_403_FORBIDDEN
from loguru import logger

from app.core.config import settings


# Security schemes - support both header and query param
api_key_header = APIKeyHeader(name="X-API-Key", auto_error=False)
api_key_query = APIKeyQuery(name="api_key", auto_error=False)


def verify_api_key(api_key: str) -> bool:
    """
    Verify an API key against the configured key.
    Uses constant-time comparison to prevent timing attacks.
    """
    if not settings.limogi_api_key:
        # No API key configured - this is a security issue
        return False

    # Constant-time comparison to prevent timing attacks
    return secrets.compare_digest(api_key, settings.limogi_api_key)


async def get_api_key(
    api_key_header: Optional[str] = Security(api_key_header),
    api_key_query: Optional[str] = Security(api_key_query),
) -> Optional[str]:
    """
    Extract API key from header or query parameter.
    Header takes precedence over query param.
    """
    return api_key_header or api_key_query


async def require_api_key(
    request: Request,
    api_key: Optional[str] = Depends(get_api_key),
) -> str:
    """
    Dependency that requires a valid API key.
    Use this on routes that need protection.

    Raises:
        HTTPException 401: If no API key provided
        HTTPException 403: If API key is invalid
    """
    # Check if authentication is enabled
    if not settings.auth_enabled:
        # Auth disabled - allow all requests (development mode)
        return "auth_disabled"

    # Check if API key is configured
    if not settings.limogi_api_key:
        logger.error("SECURITY: No LIMOGI_API_KEY configured but auth is enabled!")
        raise HTTPException(
            status_code=HTTP_403_FORBIDDEN,
            detail="Server misconfiguration: API key not set",
        )

    # Check if key was provided
    if not api_key:
        logger.warning(f"Unauthorized access attempt to {request.url.path} - no API key")
        raise HTTPException(
            status_code=HTTP_401_UNAUTHORIZED,
            detail="API key required. Provide via X-API-Key header or api_key query param.",
            headers={"WWW-Authenticate": "ApiKey"},
        )

    # Verify the key
    if not verify_api_key(api_key):
        # Log failed attempt (but don't log the actual key for security)
        logger.warning(
            f"Invalid API key attempt for {request.url.path} "
            f"from {request.client.host if request.client else 'unknown'}"
        )
        raise HTTPException(
            status_code=HTTP_403_FORBIDDEN,
            detail="Invalid API key",
        )

    return api_key


async def optional_api_key(
    api_key: Optional[str] = Depends(get_api_key),
) -> Optional[str]:
    """
    Optional API key - doesn't fail if not provided.
    Useful for routes that have different behavior for authenticated users.
    """
    if api_key and verify_api_key(api_key):
        return api_key
    return None


def generate_api_key() -> str:
    """
    Generate a cryptographically secure API key.
    Call this once to generate a key for your .env file.

    Returns:
        A 32-character hex string (128 bits of entropy)
    """
    return secrets.token_hex(16)


def hash_api_key(api_key: str) -> str:
    """
    Hash an API key for secure storage/logging.
    Never log or store the actual API key.
    """
    return hashlib.sha256(api_key.encode()).hexdigest()[:16]
