"""
Database connection and session management
"""
from sqlalchemy.ext.asyncio import create_async_engine, AsyncSession, async_sessionmaker
from sqlalchemy.orm import declarative_base, sessionmaker
from sqlalchemy import create_engine
from sqlalchemy.pool import NullPool
from app.core.config import settings
from loguru import logger

# Convert postgresql:// to postgresql+asyncpg:// for async
DATABASE_URL = settings.database_url
if DATABASE_URL.startswith("postgresql://"):
    ASYNC_DATABASE_URL = DATABASE_URL.replace("postgresql://", "postgresql+asyncpg://", 1)
else:
    ASYNC_DATABASE_URL = DATABASE_URL

# Async engine for FastAPI
async_engine = create_async_engine(
    ASYNC_DATABASE_URL,
    echo=settings.debug,
    poolclass=NullPool,
)

# Async session factory
AsyncSessionLocal = async_sessionmaker(
    bind=async_engine,
    class_=AsyncSession,
    expire_on_commit=False,
    autocommit=False,
    autoflush=False,
)

# Sync engine for background tasks that need sync operations
sync_engine = create_engine(
    DATABASE_URL,
    echo=settings.debug,
)

# Sync session factory (for shadow executor and other sync operations)
SessionLocal = sessionmaker(
    bind=sync_engine,
    autocommit=False,
    autoflush=False,
)

# Base class for ORM models
Base = declarative_base()


async def get_db() -> AsyncSession:
    """Dependency for getting async database session"""
    async with AsyncSessionLocal() as session:
        try:
            yield session
            await session.commit()
        except Exception as e:
            await session.rollback()
            logger.error(f"Database session error: {e}")
            raise
        finally:
            await session.close()


async def init_db():
    """Initialize database connection"""
    try:
        async with async_engine.begin() as conn:
            logger.info("Database connection established")
    except Exception as e:
        logger.error(f"Failed to connect to database: {e}")
        raise


async def close_db():
    """Close database connections"""
    await async_engine.dispose()
    logger.info("Database connections closed")
